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
use crate::errors;
use crate::config::{config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};

/// Global flag: set to true on Ctrl+C so subprocesses can be killed promptly.
static INTERRUPTED: AtomicBool = AtomicBool::new(false);

/// Global tokio runtime for running async subprocess code from sync context.
static RUNTIME: OnceLock<Runtime> = OnceLock::new();

/// Global channel for broadcasting interrupt signals to all running tasks.
static INTERRUPT_SENDER: OnceLock<watch::Sender<bool>> = OnceLock::new();

/// Get or initialize the global tokio runtime.
fn get_runtime() -> &'static Runtime {
    RUNTIME.get_or_init(|| {
        Runtime::new().expect(errors::TOKIO_RUNTIME)
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
    let program = cmd.get_program().to_string_lossy();
    let args: Vec<_> = cmd.get_args()
        .map(|a| a.to_string_lossy())
        .collect();
    if args.is_empty() {
        program.into_owned()
    } else {
        format!("{} {}", program, args.join(" "))
    }
}

/// If --show-child-processes is enabled, print the command that is about to be executed.
pub fn log_command(cmd: &Command) {
    if crate::runtime_flags::show_child_processes() {
        let cwd = cmd.get_current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        if cwd.is_empty() {
            eprintln!("{} {}", color::dim("[exec]"), format_command(cmd));
        } else {
            let cwd_info = format!("(in {})", cwd);
            eprintln!("{} {} {}", color::dim("[exec]"), format_command(cmd), color::dim(&cwd_info));
        }
    }
}

/// Shared inner function for running commands interruptibly using tokio.
///
/// - `inherit_stdio`: if true, inherit stdout/stderr (for --show-output mode);
///   if false, always capture via pipes.
/// - `print_on_failure`: if true, print captured output on command failure
///   (only relevant when `inherit_stdio` is false).
fn run_command_inner(cmd: &mut Command, inherit_stdio: bool, print_on_failure: bool) -> Result<Output> {
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

        if inherit_stdio {
            tokio_cmd.stdout(std::process::Stdio::inherit());
            tokio_cmd.stderr(std::process::Stdio::inherit());
        } else {
            tokio_cmd.stdout(std::process::Stdio::piped());
            tokio_cmd.stderr(std::process::Stdio::piped());
        }
        tokio_cmd.kill_on_drop(true);

        let child = tokio_cmd.spawn()
            .with_context(|| format!("Failed to spawn: {} {}",
                program.to_string_lossy(),
                args.iter().map(|a| a.to_string_lossy()).collect::<Vec<_>>().join(" ")))?;

        let mut interrupt_rx = get_interrupt_receiver();

        tokio::select! {
            biased;

            _ = interrupt_rx.changed() => {
                anyhow::bail!("Interrupted")
            }
            result = child.wait_with_output() => {
                let output = result.context("Failed to wait for child process")?;

                // Print captured output on failure if requested
                if print_on_failure && !output.status.success() {
                    if !output.stdout.is_empty() {
                        use std::io::Write;
                        let _ = std::io::stdout().write_all(&output.stdout);
                    }
                    if !output.stderr.is_empty() {
                        use std::io::Write;
                        let _ = std::io::stderr().write_all(&output.stderr);
                    }
                }

                Ok(output)
            }
        }
    })
}

/// Run a command interruptibly using tokio. If Ctrl+C is detected, the child
/// process is killed immediately via async select.
///
/// By default, output is captured and only shown on failure. Use `--show-output`
/// to always show tool output.
pub fn run_command(cmd: &mut Command) -> Result<Output> {
    let show = crate::runtime_flags::show_output();
    run_command_inner(cmd, show, !show)
}

/// Run a command and capture its stdout/stderr output.
/// Use this only when you need to parse the output.
/// For commands where output should go to terminal, use run_command() instead.
pub fn run_command_capture(cmd: &mut Command) -> Result<Output> {
    run_command_inner(cmd, false, false)
}


/// Check that a command exited successfully.
/// On failure, includes any captured stdout/stderr in the error message for debugging.
pub fn check_command_output(output: &Output, context: impl std::fmt::Display) -> Result<()> {
    if !output.status.success() {
        let mut msg = format!("{context} failed");
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        if !stderr.is_empty() {
            msg.push_str(&format!("\nstderr:\n{}", stderr.trim_end()));
        }
        if !stdout.is_empty() {
            msg.push_str(&format!("\nstdout:\n{}", stdout.trim_end()));
        }
        anyhow::bail!("{msg}");
    }
    Ok(())
}

/// Compute the scan root directory from a ScanConfig.
/// Returns empty path if scan_dir is empty, otherwise the scan_dir as a relative path.
pub fn scan_root(scan: &crate::config::ScanConfig) -> PathBuf {
    PathBuf::from(scan.scan_dir())
}

/// Check if a scan root is valid (empty means current dir, otherwise must exist).
pub fn scan_root_valid(scan: &crate::config::ScanConfig) -> bool {
    let root = scan_root(scan);
    root.as_os_str().is_empty() || root.exists()
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

/// Options for filtering sibling files in directory-based product discovery.
#[derive(Debug)]
pub struct SiblingFilter<'a> {
    pub extensions: &'a [&'a str],
    pub excludes: &'a [&'a str],
}

/// Discover directory-based products: each discovered file anchors a product whose inputs
/// include all sibling files under the same directory (filtered by extensions/excludes).
///
/// Used by processors like `make` and `cargo` where a manifest file (Makefile, Cargo.toml)
/// represents a build unit and all files in its directory are inputs.
/// All paths are relative to project root.
pub fn discover_directory_products(
    graph: &mut BuildGraph,
    scan: &crate::config::ScanConfig,
    file_index: &FileIndex,
    extra_inputs: &[String],
    cfg_hash: &impl serde::Serialize,
    siblings: &SiblingFilter,
    processor_name: &str,
) -> Result<()> {
    let files = file_index.scan(scan, true);
    if files.is_empty() {
        return Ok(());
    }

    let hash = Some(config_hash(cfg_hash));
    let extra = resolve_extra_inputs(extra_inputs)?;

    for anchor in files {
        let anchor_dir = anchor.parent().map(|p| p.to_path_buf()).unwrap_or_default();

        // Collect all matching sibling files under the anchor's directory as inputs
        let sibling_files = file_index.query(
            &anchor_dir,
            siblings.extensions,
            siblings.excludes,
            &[],
            &[],
        );

        let mut inputs: Vec<PathBuf> = Vec::new();
        // Anchor file first so product display shows it
        inputs.push(anchor.clone());
        for file in &sibling_files {
            if *file != anchor {
                inputs.push(file.clone());
            }
        }
        inputs.extend(extra.clone());
        // Empty outputs: cache entry = success record
        graph.add_product(inputs, vec![], processor_name, hash.clone())?;
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

/// Run a command in the parent directory of an anchor file (e.g., Makefile, Cargo.toml).
/// Sets `current_dir` to the parent directory (unless it's the project root).
/// Returns a display-friendly directory name for error messages.
pub fn run_in_anchor_dir(cmd: &mut Command, anchor: &Path) -> Result<Output> {
    let anchor_dir = anchor.parent()
        .context("Anchor file has no parent directory")?;
    if !anchor_dir.as_os_str().is_empty() {
        cmd.current_dir(anchor_dir);
    }
    run_command(cmd)
}

/// Format the parent directory of an anchor file for display.
/// Returns `"."` for root-level files.
pub fn anchor_display_dir(anchor: &Path) -> &str {
    anchor.parent()
        .and_then(|p| if p.as_os_str().is_empty() { None } else { p.to_str() })
        .unwrap_or(".")
}

/// Ensure a stub directory exists, creating it if necessary.
pub fn ensure_stub_dir(stub_dir: &Path, processor_name: &str) -> Result<()> {
    if !stub_dir.exists() {
        fs::create_dir_all(stub_dir)
            .with_context(|| format!("Failed to create {} stub directory", processor_name))?;
    }
    Ok(())
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
    let input_paths: Vec<&Path> = products.iter()
        .filter_map(|p| p.inputs.first().map(|i| i.as_path()))
        .collect();

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
    CargoProcessor, ClangTidyProcessor, CppcheckProcessor, MakeProcessor, MypyProcessor,
    PylintProcessor, RuffProcessor, RumdlProcessor, ShellcheckProcessor, SleepProcessor,
    SpellcheckProcessor,
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
#[derive(Debug, Clone, PartialEq)]
pub struct ProductTiming {
    pub display: String,
    pub processor: String,
    pub duration: Duration,
}

/// Statistics from processing a category of items
#[derive(Debug, Default, PartialEq)]
pub struct ProcessStats {
    pub processed: usize,
    pub failed: usize,
    pub flaky: usize,
    pub skipped: usize,
    pub restored: usize,
    pub files_created: usize,
    pub files_restored: usize,
    pub duration: Duration,
    pub product_timings: Vec<ProductTiming>,
}

impl ProcessStats {
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

    pub fn total_flaky(&self) -> usize {
        self.categories.iter().map(|s| s.flaky).sum()
    }

    pub fn print_summary(&self, summary: bool, timings: bool) {
        // Don't print human-readable summary in JSON or quiet mode
        if crate::json_output::is_json_mode() || crate::runtime_flags::quiet() {
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
            let total_flaky = self.total_flaky();
            if total_flaky > 0 {
                parts.push(format!("{} flaky", total_flaky));
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
