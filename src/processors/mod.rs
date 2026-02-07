mod checkers;
mod generators;
pub mod lua_processor;

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::Duration;
use tokio::runtime::Runtime;
use tokio::sync::watch;

use crate::color;
use crate::config::{config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;

/// Global flag: when true, print each external command before execution.
static PROCESS_DEBUG: AtomicBool = AtomicBool::new(false);

/// Global flag: set to true on Ctrl+C so subprocesses can be killed promptly.
static INTERRUPTED: AtomicBool = AtomicBool::new(false);

/// Global tokio runtime for running async subprocess code from sync context.
static RUNTIME: OnceLock<Runtime> = OnceLock::new();

/// Global channel for broadcasting interrupt signals to all running tasks.
static INTERRUPT_SENDER: OnceLock<watch::Sender<bool>> = OnceLock::new();

/// Get or initialize the global tokio runtime.
fn get_runtime() -> &'static Runtime {
    RUNTIME.get_or_init(|| {
        Runtime::new().expect("Failed to create tokio runtime")
    })
}

/// Get or initialize the interrupt channel sender.
fn get_interrupt_sender() -> &'static watch::Sender<bool> {
    INTERRUPT_SENDER.get_or_init(|| {
        let (tx, _rx) = watch::channel(false);
        tx
    })
}

/// Get a receiver for interrupt signals.
fn get_interrupt_receiver() -> watch::Receiver<bool> {
    get_interrupt_sender().subscribe()
}

/// Enable process debug logging (called once from main).
pub fn set_process_debug(enabled: bool) {
    PROCESS_DEBUG.store(enabled, Ordering::Relaxed);
}

/// Mark the global interrupted flag and notify all waiting tasks.
pub fn set_interrupted() {
    INTERRUPTED.store(true, Ordering::SeqCst);
    // Notify all tasks waiting on the interrupt channel
    let _ = get_interrupt_sender().send(true);
}

/// Check whether the global interrupted flag is set.
pub fn is_interrupted() -> bool {
    INTERRUPTED.load(Ordering::SeqCst)
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

/// Run a command interruptibly using tokio. If Ctrl+C is detected, the child
/// process is killed immediately via async select.
pub fn run_command(cmd: &mut Command) -> Result<Output> {
    log_command(cmd);

    // Check if already interrupted before spawning
    if INTERRUPTED.load(Ordering::SeqCst) {
        anyhow::bail!("Interrupted");
    }

    // Build a tokio command from std::process::Command
    let program = cmd.get_program().to_os_string();
    let args: Vec<_> = cmd.get_args().map(|a| a.to_os_string()).collect();
    let current_dir = cmd.get_current_dir().map(|p| p.to_path_buf());

    let rt = get_runtime();
    rt.block_on(async {
        let mut tokio_cmd = tokio::process::Command::new(&program);
        tokio_cmd.args(&args);
        if let Some(dir) = &current_dir {
            tokio_cmd.current_dir(dir);
        }
        tokio_cmd.stdout(std::process::Stdio::piped());
        tokio_cmd.stderr(std::process::Stdio::piped());
        // Kill child on drop to ensure cleanup
        tokio_cmd.kill_on_drop(true);

        let mut child = tokio_cmd.spawn()
            .with_context(|| format!("Failed to spawn: {} {}",
                program.to_string_lossy(),
                args.iter().map(|a| a.to_string_lossy()).collect::<Vec<_>>().join(" ")))?;

        // Take stdout/stderr handles before waiting
        let stdout_handle = child.stdout.take();
        let stderr_handle = child.stderr.take();

        let mut interrupt_rx = get_interrupt_receiver();

        tokio::select! {
            biased;

            // Check for interrupt signal first (biased)
            _ = interrupt_rx.changed() => {
                // Kill the child process (also killed on drop due to kill_on_drop)
                let _ = child.kill().await;
                anyhow::bail!("Interrupted")
            }
            // Wait for the child process to complete
            result = child.wait() => {
                let status = result.context("Failed to wait for child process")?;

                // Read stdout and stderr
                let stdout = if let Some(mut handle) = stdout_handle {
                    use tokio::io::AsyncReadExt;
                    let mut buf = Vec::new();
                    let _ = handle.read_to_end(&mut buf).await;
                    buf
                } else {
                    Vec::new()
                };

                let stderr = if let Some(mut handle) = stderr_handle {
                    use tokio::io::AsyncReadExt;
                    let mut buf = Vec::new();
                    let _ = handle.read_to_end(&mut buf).await;
                    buf
                } else {
                    Vec::new()
                };

                Ok(Output { status, stdout, stderr })
            }
        }
    })
}

pub use crate::graph::{BuildGraph, Product};

/// Check that a command exited successfully, returning an error with
/// combined stdout+stderr if it did not.
pub fn check_command_output(output: &Output, context: impl std::fmt::Display) -> Result<()> {
    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("{context} failed:\n{stdout}{stderr}");
    }
    Ok(())
}

/// Compute the scan root directory from a ScanConfig.
/// Returns empty path if scan_dir is empty, otherwise the scan_dir as a relative path.
pub fn scan_root(scan: &crate::config::ScanConfig) -> PathBuf {
    PathBuf::from(scan.scan_dir())
}

/// Compute a stub path for a source file.
/// Maps `a/b/file.ext` -> `stub_dir/a_b_file.ext.suffix`.
/// Source path is already relative to project root.
pub fn stub_path(stub_dir: &Path, source: &Path, suffix: &str) -> PathBuf {
    let stub_name = format!(
        "{}.{}",
        source.display().to_string().replace(['/', '\\'], "_"),
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
/// Used by Lua plugins that produce a single stub file per input.
/// Built-in checkers should use discover_checker_products() instead.
/// All paths are relative to project root.
#[allow(dead_code)]
pub fn discover_stub_products(
    graph: &mut BuildGraph,
    stub_dir: &Path,
    scan: &crate::config::ScanConfig,
    file_index: &FileIndex,
    extra_inputs: &[String],
    cfg_hash: &impl serde::Serialize,
    processor_name: &str,
    stub_suffix: &str,
    recursive: bool,
) -> Result<()> {
    let files = file_index.scan(scan, recursive);
    if files.is_empty() {
        return Ok(());
    }
    let hash = Some(config_hash(cfg_hash));
    let extra = resolve_extra_inputs(extra_inputs)?;
    for file in files {
        let stub = stub_path(stub_dir, &file, stub_suffix);
        let mut inputs = vec![file];
        inputs.extend(extra.clone());
        graph.add_product(inputs, vec![stub], processor_name, hash.clone())?;
    }
    Ok(())
}

/// Discover checker products: no output files, cache entry serves as success marker.
///
/// Creates one product per source file with empty outputs. When executed successfully,
/// the cache stores the input checksum. On subsequent builds, if the checksum matches,
/// the check is skipped entirely without running the external tool.
///
/// This is the standard way to implement checker discovery. See [`ProcessorType::Checker`]
/// for details on the caching behavior.
/// All paths are relative to project root.
pub fn discover_checker_products(
    graph: &mut BuildGraph,
    scan: &crate::config::ScanConfig,
    file_index: &FileIndex,
    extra_inputs: &[String],
    cfg_hash: &impl serde::Serialize,
    processor_name: &str,
) -> Result<()> {
    let files = file_index.scan(scan, true);
    if files.is_empty() {
        return Ok(());
    }
    let hash = Some(config_hash(cfg_hash));
    let extra = resolve_extra_inputs(extra_inputs)?;
    for file in files {
        let mut inputs = vec![file];
        inputs.extend(extra.clone());
        // Empty outputs: cache entry = success record
        graph.add_product(inputs, vec![], processor_name, hash.clone())?;
    }
    Ok(())
}

/// Validate that a stub product has at least one input and exactly one output.
/// Note: This is typically not needed since the graph guarantees products have inputs.
#[allow(dead_code)]
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
/// Shared helper for lint processors that support batch execution (legacy, for Lua plugins).
///
/// Runs `batch_fn` with all input paths at once. On success, writes stubs for all products.
/// On batch failure, falls back to calling `single_fn` per product to isolate errors.
#[allow(dead_code)]
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

/// Shared helper for checker processors that support batch execution (no stub files).
///
/// Runs `batch_fn` with all input paths at once. On success, returns Ok for all products.
/// On failure, the batch error is returned for all products (the tool's output shows the errors).
pub fn execute_checker_batch<F>(
    products: &[&Product],
    batch_fn: F,
) -> Vec<Result<()>>
where
    F: Fn(&[&Path]) -> Result<()>,
{
    let input_paths: Vec<&Path> = products.iter().map(|p| p.inputs[0].as_path()).collect();

    match batch_fn(&input_paths) {
        Ok(()) => products.iter().map(|_| Ok(())).collect(),
        Err(e) => {
            let err_msg = e.to_string();
            products.iter().map(|_| Err(anyhow::anyhow!("{}", err_msg))).collect()
        }
    }
}

// Re-export from subdirectories
pub use checkers::{
    CpplintProcessor, MakeProcessor, PylintProcessor, RuffProcessor,
    ShellcheckProcessor, SleepProcessor, SpellcheckProcessor,
};
pub use generators::{CcProcessor, TeraProcessor};
pub use lua_processor::LuaProcessor;

/// The type of processor - whether it generates new files or checks existing files.
///
/// # Caching Behavior
///
/// Both processor types use the cache to avoid redundant work:
///
/// - **Generators** produce output files (e.g., executables, rendered templates). The cache
///   stores copies of these outputs. On `rsb clean`, output files are deleted but the cache
///   remains intact. On the next `rsb build`, outputs are restored from cache (fast copy/hardlink)
///   instead of being regenerated.
///
/// - **Checkers** validate input files but produce no output files. The cache entry itself
///   serves as a "success marker". On `rsb clean`, there's nothing to delete. On the next
///   `rsb build`, if the cache entry exists and inputs haven't changed, the check is skipped
///   entirely (instant).
///
/// This design ensures that `rsb clean && rsb build` is fast for both types - generators
/// restore from cache, checkers skip entirely.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessorType {
    /// Generates new output files from input files (e.g., tera, cc_single_file).
    /// Products have non-empty `outputs` which are cached and can be restored.
    Generator,
    /// Checks/validates input files without producing output files (e.g., ruff, pylint, shellcheck).
    /// Products have empty `outputs`; the cache entry serves as the success marker.
    Checker,
}

impl ProcessorType {
    /// Returns the string representation
    pub fn as_str(&self) -> &'static str {
        match self {
            ProcessorType::Generator => "generator",
            ProcessorType::Checker => "checker",
        }
    }
}

/// Trait for processors that can discover products for the build graph.
///
/// Processors come in two types (see [`ProcessorType`]):
/// - **Generators**: Create output files from inputs (must override `clean()`)
/// - **Checkers**: Validate inputs without producing outputs (use default `clean()`)
///
/// # Implementing a Checker
///
/// Checkers are simpler - just implement the required methods and use defaults for the rest:
///
/// ```ignore
/// impl ProductDiscovery for MyChecker {
///     fn description(&self) -> &str { "Check files with mytool" }
///     fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex) -> Result<()> {
///         discover_checker_products(graph, ..., "mychecker")  // empty outputs
///     }
///     fn execute(&self, product: &Product) -> Result<()> {
///         run_mytool(&product.inputs[0])
///     }
///     fn auto_detect(&self, file_index: &FileIndex) -> bool { ... }
/// }
/// ```
///
/// # Implementing a Generator
///
/// Generators must override `processor_type()` and `clean()`:
///
/// ```ignore
/// impl ProductDiscovery for MyGenerator {
///     fn description(&self) -> &str { "Generate files" }
///     fn processor_type(&self) -> ProcessorType { ProcessorType::Generator }
///     fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex) -> Result<()> {
///         graph.add_product(inputs, outputs, "mygen", ...)?;  // non-empty outputs
///     }
///     fn execute(&self, product: &Product) -> Result<()> { ... }
///     fn clean(&self, product: &Product) -> Result<()> {
///         clean_outputs(product, "mygen")
///     }
///     fn auto_detect(&self, file_index: &FileIndex) -> bool { ... }
/// }
/// ```
///
/// Must be Sync + Send for parallel execution support.
pub trait ProductDiscovery: Sync + Send {
    /// Human-readable description of what this processor does
    fn description(&self) -> &str;

    /// The type of this processor (generator or checker).
    /// Default is Checker since most processors are checkers.
    fn processor_type(&self) -> ProcessorType {
        ProcessorType::Checker
    }

    /// Whether this processor should be hidden from default listings (e.g. testing-only processors)
    fn hidden(&self) -> bool {
        false
    }

    /// Discover all products this processor can produce
    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex) -> Result<()>;

    /// Discover products for clean operation (outputs only, skip expensive dependency scanning).
    /// Default implementation calls `discover()`. Override this for processors where
    /// dependency scanning is expensive (e.g., cc_single_file header scanning).
    fn discover_for_clean(&self, graph: &mut BuildGraph, file_index: &FileIndex) -> Result<()> {
        self.discover(graph, file_index)
    }

    /// Execute a single product
    fn execute(&self, product: &Product) -> Result<()>;

    /// Clean outputs for a product (called by `rsb clean`).
    ///
    /// - **Checkers**: Use the default (do nothing) - checkers have no output files.
    ///   The cache entry remains intact, so the next build will skip the check.
    ///
    /// - **Generators**: Must override to delete output files. Use `clean_outputs()`
    ///   helper. The cache entry remains intact, so the next build will restore
    ///   outputs from cache instead of regenerating them.
    fn clean(&self, _product: &Product) -> Result<()> {
        Ok(())
    }

    /// Auto-detect whether this processor is relevant for the current project
    fn auto_detect(&self, file_index: &FileIndex) -> bool;

    /// Return the names of external tools required by this processor
    fn required_tools(&self) -> Vec<String> {
        Vec::new()
    }

    /// Return tool version commands: Vec of (tool_name, args_to_get_version).
    /// Default: each required tool with `["--version"]`.
    /// Override for tools that use different flags (e.g. `-V`, `-version`, `version`).
    fn tool_version_commands(&self) -> Vec<(String, Vec<String>)> {
        self.required_tools()
            .into_iter()
            .map(|tool| (tool, vec!["--version".to_string()]))
            .collect()
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

    /// Return the processor's configuration as a JSON string for config change detection.
    /// Returns None if the processor doesn't track config (default).
    /// Processors that want config change diffs should override this.
    fn config_json(&self) -> Option<String> {
        None
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
    pub fn new() -> Self {
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
        // Don't print human-readable summary in JSON mode
        if crate::json_output::is_json_mode() {
            return;
        }

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
