mod cc;
mod cpplint;
mod make;
mod pylint;
mod ruff;
mod sleep;
mod spellcheck;
mod template;

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::color;
use crate::config::{config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;

/// Global flag: when true, print each external command before execution.
static PROCESS_DEBUG: AtomicBool = AtomicBool::new(false);

/// Enable process debug logging (called once from main).
pub fn set_process_debug(enabled: bool) {
    PROCESS_DEBUG.store(enabled, Ordering::Relaxed);
}

/// Format a `Command` as a shell-like string for display.
pub fn format_command(cmd: &Command) -> String {
    let program = cmd.get_program().to_string_lossy().to_string();
    let args: Vec<String> = cmd.get_args()
        .map(|a| a.to_string_lossy().to_string())
        .collect();
    if args.is_empty() {
        program
    } else {
        format!("{} {}", program, args.join(" "))
    }
}

/// If --process is enabled, print the command that is about to be executed.
pub fn log_command(cmd: &Command) {
    if PROCESS_DEBUG.load(Ordering::Relaxed) {
        let cwd = cmd.get_current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        if cwd.is_empty() {
            eprintln!("{} {}", color::dim("[exec]"), format_command(cmd));
        } else {
            eprintln!("{} {} {}", color::dim("[exec]"), format_command(cmd), color::dim(&format!("(in {})", cwd)));
        }
    }
}

pub use crate::graph::{BuildGraph, Product};

/// Compute the scan root directory from a ScanConfig.
/// Returns project_root if scan_dir is empty, otherwise project_root/scan_dir.
pub fn scan_root(project_root: &Path, scan: &crate::config::ScanConfig) -> PathBuf {
    let dir = scan.scan_dir();
    if dir.is_empty() {
        project_root.to_path_buf()
    } else {
        project_root.join(dir)
    }
}

/// Compute a stub path for a source file.
/// Maps `project_root/a/b/file.ext` -> `stub_dir/a_b_file.ext.suffix`.
pub fn stub_path(project_root: &Path, stub_dir: &Path, source: &Path, suffix: &str) -> PathBuf {
    let relative = source.strip_prefix(project_root).unwrap_or(source);
    let stub_name = format!(
        "{}.{}",
        relative.display().to_string().replace(['/', '\\'], "_"),
        suffix,
    );
    stub_dir.join(stub_name)
}

/// Clean outputs for a product: remove each output file and print a message.
pub fn clean_outputs(product: &Product, label: &str) -> Result<()> {
    for output in &product.outputs {
        if output.exists() {
            fs::remove_file(output)?;
            println!("Removed {} stub: {}", label, output.display());
        }
    }
    Ok(())
}

/// Discover stub-based products: one stub output per source file.
/// Used by processors that produce a single stub file per input (ruff, pylint, cpplint, spellcheck, sleep).
pub fn discover_stub_products(
    graph: &mut BuildGraph,
    project_root: &Path,
    stub_dir: &Path,
    scan: &crate::config::ScanConfig,
    file_index: &FileIndex,
    extra_inputs: &[String],
    cfg_hash: &impl serde::Serialize,
    processor_name: &str,
    stub_suffix: &str,
    recursive: bool,
) -> Result<()> {
    let files = file_index.scan(project_root, scan, recursive);
    if files.is_empty() {
        return Ok(());
    }
    let hash = Some(config_hash(cfg_hash));
    let extra = resolve_extra_inputs(project_root, extra_inputs)?;
    for file in files {
        let stub = stub_path(project_root, stub_dir, &file, stub_suffix);
        let mut inputs = vec![file];
        inputs.extend(extra.clone());
        graph.add_product(inputs, vec![stub], processor_name, hash.clone())?;
    }
    Ok(())
}

/// Validate that a stub product has at least one input and exactly one output.
pub fn validate_stub_product(product: &Product, processor_name: &str) -> Result<()> {
    if product.inputs.is_empty() || product.outputs.len() != 1 {
        anyhow::bail!("{} product must have at least one input and exactly one output", processor_name);
    }
    Ok(())
}

/// Ensure a stub directory exists, creating it if necessary.
pub fn ensure_stub_dir(stub_dir: &Path, processor_name: &str) -> Result<()> {
    if !stub_dir.exists() {
        fs::create_dir_all(stub_dir)
            .context(format!("Failed to create {} stub directory", processor_name))?;
    }
    Ok(())
}

/// Create a stub file with the given content after a successful processor run.
pub fn write_stub(stub_path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = stub_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(stub_path, content).context("Failed to create stub file")?;
    Ok(())
}
/// Shared helper for lint processors that support batch execution.
///
/// Runs `batch_fn` with all input paths at once. On success, writes stubs for all products.
/// On batch failure, falls back to calling `single_fn` per product to isolate errors.
pub fn execute_lint_batch<F, G>(
    products: &[&Product],
    processor_name: &str,
    stub_dir: &Path,
    batch_fn: F,
    single_fn: G,
) -> Vec<Result<()>>
where
    F: Fn(&[&Path]) -> Result<()>,
    G: Fn(&Path, &Path) -> Result<()>,
{
    // Validate all products up front and collect input/stub pairs
    let mut validated: Vec<(&Path, &Path)> = Vec::with_capacity(products.len());
    let mut results: Vec<Option<Result<()>>> = (0..products.len()).map(|_| None).collect();

    for (i, product) in products.iter().enumerate() {
        if let Err(e) = validate_stub_product(product, processor_name) {
            results[i] = Some(Err(e));
        } else {
            validated.push((&product.inputs[0], &product.outputs[0]));
        }
    }

    // Ensure stub directory exists
    if let Err(e) = ensure_stub_dir(stub_dir, processor_name) {
        // If we can't create the stub dir, all products fail
        return products.iter().enumerate().map(|(i, _)| {
            results[i].take().unwrap_or_else(|| Err(anyhow::anyhow!("{}", e)))
        }).collect();
    }

    // Collect only the input paths for validated products
    let input_paths: Vec<&Path> = validated.iter().map(|(input, _)| *input).collect();

    if input_paths.is_empty() {
        // All products failed validation
        return results.into_iter().map(|r| r.unwrap()).collect();
    }

    // Try batch execution
    if batch_fn(&input_paths).is_ok() {
        // Batch succeeded — write stubs for all validated products
        let mut validated_iter = validated.iter();
        for (i, _product) in products.iter().enumerate() {
            if results[i].is_some() {
                continue; // Already failed validation
            }
            let (_input, stub) = validated_iter.next().unwrap();
            results[i] = Some(write_stub(stub, "linted"));
        }
    } else {
        // Batch failed — fall back to per-file execution to isolate errors
        let mut validated_iter = validated.iter();
        for (i, _product) in products.iter().enumerate() {
            if results[i].is_some() {
                continue; // Already failed validation
            }
            let (input, stub) = validated_iter.next().unwrap();
            results[i] = Some(single_fn(input, stub));
        }
    }

    results.into_iter().map(|r| r.unwrap()).collect()
}

pub use cc::CcProcessor;
pub use cpplint::Cpplinter;
pub use make::MakeProcessor;
pub use pylint::PylintProcessor;
pub use ruff::RuffProcessor;
pub use sleep::SleepProcessor;
pub use spellcheck::SpellcheckProcessor;
pub use template::TemplateProcessor;

/// Trait for processors that can discover products for the build graph
/// Must be Sync + Send for parallel execution support
pub trait ProductDiscovery: Sync + Send {
    /// Discover all products this processor can produce
    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex) -> Result<()>;

    /// Execute a single product
    fn execute(&self, product: &Product) -> Result<()>;

    /// Clean outputs for a product
    fn clean(&self, product: &Product) -> Result<()>;

    /// Auto-detect whether this processor is relevant for the current project
    fn auto_detect(&self, file_index: &FileIndex) -> bool;

    /// Return the names of external tools required by this processor
    fn required_tools(&self) -> Vec<String> {
        Vec::new()
    }

    /// Whether this processor supports batch execution of multiple products at once.
    fn supports_batch(&self) -> bool {
        false
    }

    /// Execute multiple products in one invocation.
    /// Returns one Result per product, in the same order as the input.
    /// Default: falls back to per-product execute().
    fn execute_batch(&self, products: &[&Product]) -> Vec<Result<()>> {
        products.iter().map(|p| self.execute(p)).collect()
    }
}

/// Timing for a single product execution
#[derive(Debug, Clone)]
pub struct ProductTiming {
    pub display: String,
    pub processor: String,
    pub duration: Duration,
}

/// Statistics from processing a category of items
#[derive(Debug)]
pub struct ProcessStats {
    pub processed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub restored: usize,
    pub files_created: usize,
    pub files_restored: usize,
    pub duration: Duration,
    pub product_timings: Vec<ProductTiming>,
}

impl ProcessStats {
    pub fn new(_name: &str) -> Self {
        Self {
            processed: 0,
            failed: 0,
            skipped: 0,
            restored: 0,
            files_created: 0,
            files_restored: 0,
            duration: Duration::ZERO,
            product_timings: Vec::new(),
        }
    }

    pub fn total(&self) -> usize {
        self.processed + self.failed + self.skipped + self.restored
    }
}

/// Aggregated statistics from all processors
#[derive(Default)]
pub struct BuildStats {
    pub categories: Vec<ProcessStats>,
    pub total_duration: Duration,
    pub failed_count: usize,
    pub failed_messages: Vec<String>,
}

impl BuildStats {
    pub fn add(&mut self, stats: ProcessStats) {
        if stats.total() > 0 {
            self.categories.push(stats);
        }
    }

    pub fn total_processed(&self) -> usize {
        self.categories.iter().map(|s| s.processed).sum()
    }

    pub fn total_skipped(&self) -> usize {
        self.categories.iter().map(|s| s.skipped).sum()
    }

    pub fn total_restored(&self) -> usize {
        self.categories.iter().map(|s| s.restored).sum()
    }

    pub fn total_files_created(&self) -> usize {
        self.categories.iter().map(|s| s.files_created).sum()
    }

    pub fn total_files_restored(&self) -> usize {
        self.categories.iter().map(|s| s.files_restored).sum()
    }

    pub fn print_summary(&self, summary: bool, timings: bool) {
        if !summary && !timings {
            return;
        }

        if self.categories.is_empty() && self.failed_count == 0 {
            if summary {
                println!("{}", color::dim("Nothing to build."));
            }
            return;
        }

        if summary {
            let total_processed = self.total_processed();
            let total_restored = self.total_restored();
            let total_failed = self.failed_count;
            let total_skipped = self.total_skipped();
            let total_files_created = self.total_files_created();
            let total_files_restored = self.total_files_restored();

            let mut parts = Vec::new();
            if total_processed > 0 {
                if total_files_created > 0 {
                    parts.push(format!("{} processed ({} files created)", total_processed, total_files_created));
                } else {
                    parts.push(format!("{} processed", total_processed));
                }
            }
            if total_restored > 0 {
                if total_files_restored > 0 {
                    parts.push(format!("{} restored ({} files)", total_restored, total_files_restored));
                } else {
                    parts.push(format!("{} restored", total_restored));
                }
            }
            if total_failed > 0 {
                parts.push(format!("{} failed", total_failed));
            }
            if total_skipped > 0 {
                parts.push(format!("{} unchanged", total_skipped));
            }

            if parts.is_empty() {
                println!("{}", color::dim("Nothing to build."));
            } else {
                let line = format!("Build summary: {}", parts.join(", "));
                if total_failed > 0 {
                    println!("{}", color::red(&line));
                } else {
                    println!("{}", color::green(&line));
                }
            }
        }

        if self.failed_count > 0 {
            println!("{}", color::red(&format!("Build finished with {} error(s):", self.failed_count)));
            for msg in &self.failed_messages {
                println!("{} {}", color::red("*"), msg);
            }
        }

        if timings {
            println!();
            println!("{}", color::bold("Timing:"));
            for cat in &self.categories {
                for pt in &cat.product_timings {
                    println!("[{}] {} {}", pt.processor, pt.display,
                        color::dim(&format!("({:.3}s)", pt.duration.as_secs_f64())));
                }
            }
            println!("{}", color::bold(&format!("Total: {:.3}s", self.total_duration.as_secs_f64())));
        }
    }
}
