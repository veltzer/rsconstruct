mod cc;
mod cpplint;
mod pylint;
mod ruff;
mod sleep;
mod spellcheck;
mod template;

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use walkdir::WalkDir;

use crate::color;
use crate::config::{ScanConfig, config_hash, resolve_extra_inputs};
use crate::ignore::IgnoreRules;

pub use crate::graph::{BuildGraph, Product};

/// Find files matching given extensions in a directory.
///
/// - `root`: directory to walk
/// - `extensions`: file extensions to match (e.g., &[".py", ".pyi"])
/// - `exclude_dirs`: directory path segments to skip (e.g., &["/.git/", "/out/"])
/// - `ignore_rules`: project ignore rules
/// - `recursive`: if false, only scan the top-level directory
///
/// Returns sorted list of matching paths.
pub fn find_files(
    root: &Path,
    extensions: &[&str],
    exclude_dirs: &[&str],
    ignore_rules: &Arc<IgnoreRules>,
    recursive: bool,
) -> Vec<PathBuf> {
    if !root.exists() {
        return Vec::new();
    }

    let walker = if recursive {
        WalkDir::new(root)
    } else {
        WalkDir::new(root).max_depth(1)
    };

    let mut files: Vec<PathBuf> = walker
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            let path = e.path();
            if !path.is_file() {
                return false;
            }

            // Check exclude dirs
            if !exclude_dirs.is_empty() {
                let path_str = path.to_string_lossy();
                if exclude_dirs.iter().any(|dir| path_str.contains(dir)) {
                    return false;
                }
            }

            // Check extension match
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !extensions.iter().any(|ext| name.ends_with(ext)) {
                return false;
            }

            // Check ignore rules
            !ignore_rules.is_ignored(path)
        })
        .map(|e| e.path().to_path_buf())
        .collect();

    files.sort();
    files
}

/// Compute the scan root directory from a ScanConfig.
/// Returns project_root if scan_dir is empty, otherwise project_root/scan_dir.
pub fn scan_root(project_root: &Path, scan: &ScanConfig) -> PathBuf {
    let dir = scan.scan_dir();
    if dir.is_empty() {
        project_root.to_path_buf()
    } else {
        project_root.join(dir)
    }
}

/// Find files using a resolved ScanConfig.
/// Combines scan_root + find_files with the config's extensions and exclude_dirs.
pub fn scan_files(
    project_root: &Path,
    scan: &ScanConfig,
    ignore_rules: &Arc<IgnoreRules>,
    recursive: bool,
) -> Vec<PathBuf> {
    let root = scan_root(project_root, scan);
    let ext_refs: Vec<&str> = scan.extensions().iter().map(|s| s.as_str()).collect();
    let exclude_refs: Vec<&str> = scan.exclude_dirs().iter().map(|s| s.as_str()).collect();
    find_files(&root, &ext_refs, &exclude_refs, ignore_rules, recursive)
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
    scan: &ScanConfig,
    ignore_rules: &Arc<IgnoreRules>,
    extra_inputs: &[String],
    cfg_hash: &impl serde::Serialize,
    processor_name: &str,
    stub_suffix: &str,
    recursive: bool,
) -> Result<()> {
    let files = scan_files(project_root, scan, ignore_rules, recursive);
    if files.is_empty() {
        return Ok(());
    }
    let hash = Some(config_hash(cfg_hash));
    let extra = resolve_extra_inputs(project_root, extra_inputs)?;
    for file in files {
        let stub = stub_path(project_root, stub_dir, &file, stub_suffix);
        let mut inputs = vec![file];
        inputs.extend(extra.clone());
        graph.add_product(inputs, vec![stub], processor_name, hash.clone());
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
pub use cc::CcProcessor;
pub use cpplint::Cpplinter;
pub use pylint::PylintProcessor;
pub use ruff::RuffProcessor;
pub use sleep::SleepProcessor;
pub use spellcheck::SpellcheckProcessor;
pub use template::TemplateProcessor;

/// Trait for processors that can discover products for the build graph
/// Must be Sync + Send for parallel execution support
pub trait ProductDiscovery: Sync + Send {
    /// Discover all products this processor can produce
    fn discover(&self, graph: &mut BuildGraph) -> Result<()>;

    /// Execute a single product
    fn execute(&self, product: &Product) -> Result<()>;

    /// Clean outputs for a product
    fn clean(&self, product: &Product) -> Result<()>;

    /// Auto-detect whether this processor is relevant for the current project
    fn auto_detect(&self) -> bool;

    /// Return the names of external tools required by this processor
    fn required_tools(&self) -> Vec<String> {
        Vec::new()
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
