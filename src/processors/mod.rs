mod checkers;
mod explicit;
pub(crate) mod generators;
mod creators;
pub mod lua;

use anyhow::{Context, Result};
use serde::Serialize;
#[cfg(debug_assertions)]
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::Duration;
use tokio::runtime::Runtime;
use tokio::sync::watch;

use crate::color;
use crate::errors;
use crate::config::{
    output_config_hash, resolve_extra_inputs,
    CheckerConfigWithCommand, SimpleCheckerParams, StandardConfig,
};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};

/// Processor name constants — auto-generated from the central registry in `src/registry.rs`.
pub mod names {
    pub const TERA: &str = "tera";
    pub const CC_SINGLE_FILE: &str = "cc_single_file";
    pub const CC: &str = "cc";
    pub const ZSPELL: &str = "zspell";
    pub const MAKE: &str = "make";
    pub const CARGO: &str = "cargo";
    pub const CLIPPY: &str = "clippy";
    pub const TAGS: &str = "tags";
    pub const PIP: &str = "pip";
    pub const SPHINX: &str = "sphinx";
    pub const MDBOOK: &str = "mdbook";
    pub const NPM: &str = "npm";
    pub const GEM: &str = "gem";
    pub const MDL: &str = "mdl";
    pub const MARKDOWNLINT: &str = "markdownlint";
    pub const ASPELL: &str = "aspell";
    pub const PDFLATEX: &str = "pdflatex";
    pub const MAKO: &str = "mako";
    pub const JINJA2: &str = "jinja2";
    pub const PDFUNITE: &str = "pdfunite";
    pub const IPDFUNITE: &str = "ipdfunite";
    pub const SCRIPT: &str = "script";
    pub const GENERATOR: &str = "generator";
    pub const EXPLICIT: &str = "explicit";
    pub const LINUX_MODULE: &str = "linux_module";
    pub const JEKYLL: &str = "jekyll";
    pub const RUST_SINGLE_FILE: &str = "rust_single_file";
}

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

/// Resolve a relative path against an anchor directory.
/// If the anchor directory is empty, the relative path is returned as-is.
pub(crate) fn resolve_anchor_path(anchor_dir: &Path, rel: &str) -> PathBuf {
    if anchor_dir.as_os_str().is_empty() {
        PathBuf::from(rel)
    } else {
        anchor_dir.join(rel)
    }
}

/// Mark the global interrupted flag and notify all waiting tasks.
pub(crate) fn set_interrupted() {
    INTERRUPTED.store(true, Ordering::SeqCst);
    // Notify all tasks waiting on the interrupt channel
    let _ = get_interrupt_sender().send(true);
}


// Thread-local holding the current processor's declared tools.
// Set before execute()/execute_batch() and cleared after.
// Used by debug_assert in run_command_inner() to catch undeclared tool usage.
#[cfg(debug_assertions)]
thread_local! {
    static DECLARED_TOOLS: RefCell<Option<Vec<String>>> = const { RefCell::new(None) };
}

/// Set the declared tools for the current thread (debug builds only).
#[cfg(debug_assertions)]
pub(crate) fn set_declared_tools(tools: Option<Vec<String>>) {
    DECLARED_TOOLS.with(|dt| {
        *dt.borrow_mut() = tools;
    });
}

/// No-op in release builds.
#[cfg(not(debug_assertions))]
pub(crate) fn set_declared_tools(_tools: Option<Vec<String>>) {}

/// Temporarily suspend the declared-tools check for user-specified commands.
/// Returns a guard that restores the previous value when dropped.
#[cfg(debug_assertions)]
pub(crate) fn suspend_tool_check() -> ToolCheckGuard {
    let prev = DECLARED_TOOLS.with(|dt| dt.borrow_mut().take());
    ToolCheckGuard { prev }
}

/// No-op in release builds.
#[cfg(not(debug_assertions))]
pub(crate) fn suspend_tool_check() -> ToolCheckGuard {
    ToolCheckGuard { _private: () }
}

/// RAII guard that restores the declared tools when dropped.
#[cfg(debug_assertions)]
pub(crate) struct ToolCheckGuard {
    prev: Option<Vec<String>>,
}

#[cfg(debug_assertions)]
impl Drop for ToolCheckGuard {
    fn drop(&mut self) {
        DECLARED_TOOLS.with(|dt| {
            *dt.borrow_mut() = self.prev.take();
        });
    }
}

/// No-op guard for release builds.
#[cfg(not(debug_assertions))]
pub(crate) struct ToolCheckGuard {
    _private: (),
}

/// Format a `Command` as a shell-like string for display.
pub(crate) fn format_command(cmd: &Command) -> String {
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
pub(crate) fn log_command(cmd: &Command) {
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
fn run_command_inner(cmd: &mut Command, inherit_stdio: bool) -> Result<Output> {
    log_command(cmd);

    #[cfg(debug_assertions)]
    DECLARED_TOOLS.with(|dt| {
        if let Some(ref tools) = *dt.borrow() {
            let program = cmd.get_program().to_string_lossy();
            let basename = program.rsplit('/').next().unwrap_or(&program);
            assert!(
                tools.iter().any(|t| {
                    let t_basename = t.rsplit('/').next().unwrap_or(t);
                    t_basename == basename
                }),
                "Processor executed undeclared tool '{basename}'. Declared: {tools:?}",
            );
        }
    });

    // Check if already interrupted before spawning
    if INTERRUPTED.load(Ordering::SeqCst) {
        anyhow::bail!("Interrupted");
    }

    // Build a tokio command from std::process::Command
    let program = cmd.get_program().to_os_string();
    let args: Vec<_> = cmd.get_args().map(|a| a.to_os_string()).collect();
    let current_dir = cmd.get_current_dir().map(|p| p.to_path_buf());
    let envs: Vec<_> = cmd.get_envs()
        .filter_map(|(k, v)| v.map(|val| (k.to_os_string(), val.to_os_string())))
        .collect();

    let rt = get_runtime();
    rt.block_on(async {
        let mut tokio_cmd = tokio::process::Command::new(&program);
        tokio_cmd.args(&args);
        if let Some(dir) = &current_dir {
            tokio_cmd.current_dir(dir);
        }
        for (key, val) in &envs {
            tokio_cmd.env(key, val);
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
            .with_context(|| {
                let prog = program.to_string_lossy();
                let total_len: usize = prog.len() + args.iter().map(|a| a.len() + 1).sum::<usize>();
                let arg_count = args.len();
                if total_len > 100_000 {
                    format!(
                        "Failed to spawn '{}' with {} arguments (total command length ~{} bytes). \
                         This usually means the argument list is too long for the OS (E2BIG). \
                         Consider reducing the number of files or excluding directories.",
                        prog, arg_count, total_len
                    )
                } else {
                    format!("Failed to spawn: {} {}", prog,
                        args.iter().map(|a| a.to_string_lossy()).collect::<Vec<_>>().join(" "))
                }
            })?;

        let mut interrupt_rx = get_interrupt_receiver();

        // Close race window: re-check after subscribing, since an interrupt
        // between the INTERRUPTED check above and subscribe() would be missed.
        if INTERRUPTED.load(Ordering::SeqCst) {
            anyhow::bail!("Interrupted");
        }

        tokio::select! {
            biased;

            _ = interrupt_rx.changed() => {
                anyhow::bail!("Interrupted")
            }
            result = child.wait_with_output() => {
                let output = crate::errors::ctx(result, "Failed to wait for child process")?;
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
pub(crate) fn run_command(cmd: &mut Command) -> Result<Output> {
    let show = crate::runtime_flags::show_output();
    run_command_inner(cmd, show)
}

/// Run a command and capture its stdout/stderr output.
/// Use this only when you need to parse the output.
/// For commands where output should go to terminal, use run_command() instead.
pub(crate) fn run_command_capture(cmd: &mut Command) -> Result<Output> {
    run_command_inner(cmd, false)
}


/// Check that a command exited successfully.
/// On failure, includes any captured stdout/stderr in the error message for debugging.
pub(crate) fn check_command_output(output: &Output, context: impl std::fmt::Display) -> Result<()> {
    if !output.status.success() {
        use std::fmt::Write;
        let mut msg = format!("{context} failed");
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        if !stderr.is_empty() {
            let _ = write!(msg, "\nstderr:\n{}", stderr.trim_end());
        }
        if !stdout.is_empty() {
            let _ = write!(msg, "\nstdout:\n{}", stdout.trim_end());
        }
        anyhow::bail!("{msg}");
    }
    Ok(())
}

/// Check if all scan roots are valid (empty means current dir, otherwise must exist).
/// Check if scan directories are valid. Always returns true because scan directories
/// may not exist on disk yet but contain virtual files from the fixed-point discovery
/// loop (upstream generator outputs). The actual filtering is done by `file_index.scan()`.
pub(crate) fn scan_root_valid(_scan: &crate::config::StandardConfig) -> bool {
    true
}

/// Compute a stub path for a source file.
/// Maps `a/b/file.ext` -> `stub_dir/a_b_file.ext.suffix`.
/// Source path is already relative to project root.
pub(crate) fn stub_path(stub_dir: &Path, source: &Path, suffix: &str) -> PathBuf {
    let stub_name = format!(
        "{}.{}",
        source.display().to_string().replace(['/', '\\'], "_"),
        suffix,
    );
    stub_dir.join(stub_name)
}

/// Convert a DOT graph string to SVG using the `dot` command.
pub(crate) fn dot_to_svg(dot_content: &str) -> Result<String> {
    use std::process::{Command, Stdio};
    let mut cmd = Command::new("dot");
    cmd.arg("-Tsvg")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    log_command(&cmd);
    let mut child = cmd
        .spawn()
        .map_err(|_| anyhow::anyhow!("Graphviz 'dot' command not found. Install Graphviz to use SVG format"))?;
    child.stdin.take()
        .context("stdin was not piped to dot command")?
        .write_all(dot_content.as_bytes())?;
    let output = child.wait_with_output()?;
    check_command_output(&output, "dot")?;
    Ok(String::from_utf8(output.stdout)?)
}

/// Append new words to a words file without truncating existing content.
/// Used by aspell and zspell processors for their auto_add_words feature.
/// `existing` is the set of words already on disk, `new_words` the words to add.
/// If `header_line` is Some and the file does not yet exist, it is written as the
/// first line (e.g. aspell .pws header). New words are appended to the end of the
/// file so that existing content is never lost.
pub(crate) fn flush_words(
    existing: &HashSet<String>,
    new_words: &HashSet<String>,
    words_path: &Path,
    header_line: Option<&str>,
) -> Result<()> {
    let to_add: Vec<_> = new_words.iter()
        .filter(|w| !existing.contains(*w))
        .collect();
    if to_add.is_empty() {
        return Ok(());
    }
    let mut sorted: Vec<_> = to_add;
    sorted.sort();

    // If the file doesn't exist yet, create it with the header line.
    // Otherwise just append — never truncate.
    let file_exists = words_path.exists();
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(words_path)
        .with_context(|| format!("Failed to open words file: {}", words_path.display()))?;
    if !file_exists
        && let Some(header) = header_line {
            writeln!(file, "{}", header)?;
    }
    for word in &sorted {
        writeln!(file, "{}", word)?;
    }
    println!("Added {} word(s) to {}", sorted.len(), words_path.display());
    Ok(())
}

/// Check if a config file exists and return it as extra inputs for discover.
/// Used by processors that auto-detect config files (e.g. mypy.ini, .pylintrc).
pub(crate) fn config_file_inputs(path: &str) -> Vec<String> {
    if Path::new(path).exists() {
        vec![path.to_string()]
    } else {
        Vec::new()
    }
}

/// Create the parent directory of an output path if it doesn't exist.
/// Used by generator processors before writing output files.
pub(crate) fn ensure_output_dir(output: &Path) -> Result<()> {
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create output directory: {}", parent.display()))?;
    }
    Ok(())
}

/// Remove the output_dirs of a product. Used by creator clean() methods.
/// Returns the number of directories removed.
pub(crate) fn clean_output_dir(product: &Product, processor_name: &str, verbose: bool) -> Result<usize> {
    let mut count = 0;
    for output_dir in &product.output_dirs {
        if output_dir.exists() {
            if verbose {
                println!("Removing {} output directory: {}", processor_name, output_dir.display());
            }
            crate::errors::ctx(fs::remove_dir_all(output_dir.as_ref()), &format!("Failed to remove output directory: {}", output_dir.display()))?;
            count += 1;
        }
    }
    Ok(count)
}

/// Build the input list for creators: anchor first, then sibling files
/// (excluding the anchor to avoid duplicates), then extra inputs.
pub(crate) fn build_anchor_inputs(anchor: &Path, sibling_files: &[PathBuf], extra: &[PathBuf]) -> Vec<PathBuf> {
    let mut inputs: Vec<PathBuf> = Vec::with_capacity(1 + sibling_files.len() + extra.len());
    inputs.push(anchor.to_path_buf());
    for file in sibling_files {
        if *file != anchor {
            inputs.push(file.clone());
        }
    }
    inputs.extend_from_slice(extra);
    inputs
}

/// Combine the scan_root_valid check, scan, and empty check that creators
/// repeat in their discover() methods. Returns None if the scan root is invalid
/// or no files were found, otherwise returns the list of files.
pub(crate) fn scan_or_skip(scan: &crate::config::StandardConfig, file_index: &FileIndex) -> Option<Vec<PathBuf>> {
    if !scan_root_valid(scan) {
        return None;
    }
    let files = file_index.scan(scan, true);
    if files.is_empty() {
        return None;
    }
    Some(files)
}

/// Clean outputs for a product: remove each output file.
/// When `verbose` is true, prints a message for each removed file.
/// Returns the number of files removed.
pub(crate) fn clean_outputs(product: &Product, label: &str, verbose: bool) -> Result<usize> {
    let mut count = 0;
    for output in &product.outputs {
        match fs::remove_file(output) {
            Ok(()) => {
                count += 1;
                if verbose {
                    println!("Removed {} output: {}", label, output.display());
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(count)
}

/// Options for filtering sibling files in directory-based product discovery.
#[derive(Debug)]
pub(crate) struct SiblingFilter<'a> {
    pub extensions: &'a [&'a str],
    pub excludes: &'a [&'a str],
}

/// Options for `discover_directory_products`.
pub(crate) struct DirectoryProductOpts<'a, H: serde::Serialize> {
    pub scan: &'a crate::config::StandardConfig,
    pub file_index: &'a FileIndex,
    pub dep_inputs: &'a [String],
    pub cfg_hash: &'a H,
    pub siblings: &'a SiblingFilter<'a>,
    pub processor_name: &'a str,
    pub output_dir_name: Option<&'a str>,
}

/// Discover directory-based products: each discovered file anchors a product whose inputs
/// include all sibling files under the same directory (filtered by extensions/excludes).
///
/// Used by processors like `make` and `cargo` where a manifest file (Makefile, Cargo.toml)
/// represents a build unit and all files in its directory are inputs.
/// All paths are relative to project root.
///
/// When `output_dir_name` is `Some("dir_name")`, the product gets an `output_dir` set to
/// `anchor_parent/dir_name`, enabling directory-level caching for creators.
pub(crate) fn discover_directory_products(
    graph: &mut BuildGraph,
    opts: DirectoryProductOpts<'_, impl serde::Serialize>,
) -> Result<()> {
    let DirectoryProductOpts { scan, file_index, dep_inputs, cfg_hash, siblings, processor_name, output_dir_name } = opts;
    let files = file_index.scan(scan, true);
    if files.is_empty() {
        return Ok(());
    }

    let hash = Some(output_config_hash(cfg_hash, &[]));
    let extra = resolve_extra_inputs(dep_inputs)?;

    for anchor in files {
        let anchor_dir = anchor.parent().map(|p| p.to_path_buf()).unwrap_or_default();

        // Collect all matching sibling files under the anchor's directory as inputs
        let sibling_files = file_index.query(
            &anchor_dir,
            siblings.extensions,
            siblings.excludes,
            &[],
            &[],
            &[],
        );

        let inputs = build_anchor_inputs(&anchor, &sibling_files, &extra);

        if let Some(dir_name) = output_dir_name {
            let output_dir = if anchor_dir.as_os_str().is_empty() {
                PathBuf::from(dir_name)
            } else {
                anchor_dir.join(dir_name)
            };
            graph.add_product_with_output_dir(inputs, vec![], processor_name, hash.clone(), output_dir)?;
        } else {
            // Empty outputs: cache entry = success record
            graph.add_product(inputs, vec![], processor_name, hash.clone())?;
        }
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
pub(crate) fn discover_checker_products(
    graph: &mut BuildGraph,
    scan: &crate::config::StandardConfig,
    file_index: &FileIndex,
    dep_inputs: &[String],
    cfg_hash: &impl serde::Serialize,
    processor_name: &str,
) -> Result<()> {
    let files = file_index.scan(scan, true);
    if files.is_empty() {
        return Ok(());
    }
    let hash = Some(output_config_hash(cfg_hash, &[]));
    let extra = resolve_extra_inputs(dep_inputs)?;
    for file in files {
        let mut inputs = Vec::with_capacity(1 + extra.len());
        inputs.push(file);
        inputs.extend_from_slice(&extra);
        // Empty outputs: cache entry = success record
        graph.add_product(inputs, vec![], processor_name, hash.clone())?;
    }
    Ok(())
}

/// Standard checker discover: merge dep_inputs + dep_auto, then call discover_checker_products.
/// Used by all checkers that follow the standard discover pattern.
pub(crate) fn checker_discover(
    graph: &mut BuildGraph,
    scan: &crate::config::StandardConfig,
    file_index: &FileIndex,
    dep_inputs: &[String],
    dep_auto: &[String],
    cfg_hash: &impl serde::Serialize,
    processor_name: &str,
) -> Result<()> {
    let mut all_dep_inputs = dep_inputs.to_vec();
    for ai in dep_auto {
        all_dep_inputs.extend(config_file_inputs(ai));
    }
    discover_checker_products(graph, scan, file_index, &all_dep_inputs, cfg_hash, processor_name)
}

/// Standard checker auto_detect: check if scan finds any files.
pub(crate) fn checker_auto_detect(scan: &crate::config::StandardConfig, file_index: &FileIndex) -> bool {
    !file_index.scan(scan, true).is_empty()
}

/// Standard checker auto_detect with scan_root guard.
pub(crate) fn checker_auto_detect_with_scan_root(scan: &crate::config::StandardConfig, file_index: &FileIndex) -> bool {
    scan_root_valid(scan) && !file_index.scan(scan, true).is_empty()
}

/// Run a command in the parent directory of an anchor file (e.g., Makefile, Cargo.toml).
/// Sets `current_dir` to the parent directory (unless it's the project root).
/// Returns a display-friendly directory name for error messages.
pub(crate) fn run_in_anchor_dir(cmd: &mut Command, anchor: &Path) -> Result<Output> {
    let anchor_dir = anchor.parent()
        .context("Anchor file has no parent directory")?;
    if !anchor_dir.as_os_str().is_empty() {
        cmd.current_dir(anchor_dir);
    }
    run_command(cmd)
}

/// Format the parent directory of an anchor file for display.
/// Returns `"."` for root-level files.
pub(crate) fn anchor_display_dir(anchor: &Path) -> &str {
    anchor.parent()
        .and_then(|p| if p.as_os_str().is_empty() { None } else { p.to_str() })
        .unwrap_or(".")
}

/// Ensure a stub directory exists, creating it if necessary.
pub(crate) fn ensure_stub_dir(stub_dir: &Path, processor_name: &str) -> Result<()> {
    if !stub_dir.exists() {
        fs::create_dir_all(stub_dir)
            .with_context(|| format!("Failed to create {} stub directory", processor_name))?;
    }
    Ok(())
}

/// Run a checker tool on one or more files.
///
/// Builds a command from the tool name, optional subcommand, config args, and file paths,
/// then runs it and checks the output.
/// Maximum total argument length (in bytes) before splitting into multiple invocations.
/// Linux limit is typically ~2MB; we use a conservative threshold to leave headroom
/// for environment variables and the tool path itself.
const MAX_ARG_LEN: usize = 1_000_000;

pub(crate) fn run_checker(
    tool: &str,
    subcommand: Option<&str>,
    args: &[String],
    files: &[&Path],
) -> Result<()> {
    // Deduplicate files — the same file can appear in multiple products
    // (e.g., a script that is both a normal scan result and a generator dependency)
    let mut files: Vec<&Path> = files.to_vec();
    files.sort();
    files.dedup();
    let files = &files[..];

    // Calculate the base command length (tool + subcommand + args)
    let base_len: usize = tool.len()
        + subcommand.map_or(0, |s| s.len() + 1)
        + args.iter().map(|a| a.len() + 1).sum::<usize>();

    // Check if all files fit in a single invocation
    let files_len: usize = files.iter().map(|f| f.as_os_str().len() + 1).sum();
    if base_len + files_len <= MAX_ARG_LEN {
        return run_checker_once(tool, subcommand, args, files);
    }

    // Split files into chunks that fit within the limit
    let mut chunk_start = 0;
    while chunk_start < files.len() {
        let mut chunk_len = base_len;
        let mut chunk_end = chunk_start;
        while chunk_end < files.len() {
            let file_len = files[chunk_end].as_os_str().len() + 1;
            if chunk_len + file_len > MAX_ARG_LEN && chunk_end > chunk_start {
                break;
            }
            chunk_len += file_len;
            chunk_end += 1;
        }
        run_checker_once(tool, subcommand, args, &files[chunk_start..chunk_end])?;
        chunk_start = chunk_end;
    }
    Ok(())
}

fn run_checker_once(
    tool: &str,
    subcommand: Option<&str>,
    args: &[String],
    files: &[&Path],
) -> Result<()> {
    let mut cmd = Command::new(tool);
    if let Some(sub) = subcommand {
        cmd.arg(sub);
    }
    for arg in args {
        cmd.arg(arg);
    }
    for file in files {
        cmd.arg(file);
    }
    let output = run_command(&mut cmd)?;
    check_command_output(&output, tool)
}

/// Shared helper for checker processors that support batch execution (no stub files).
///
/// Runs `batch_fn` with all input paths at once. On success, returns Ok for all products.
/// On failure, the batch error is returned for all products (the tool's output shows the errors).
pub(crate) fn execute_checker_batch<F>(
    products: &[&Product],
    batch_fn: F,
) -> Vec<Result<()>>
where
    F: Fn(&[&Path]) -> Result<()>,
{
    let input_paths: Vec<&Path> = products.iter()
        .map(|p| p.primary_input())
        .collect();

    match batch_fn(&input_paths) {
        Ok(()) => products.iter().map(|_| Ok(())).collect(),
        Err(e) => {
            let err_msg = e.to_string();
            products.iter().map(|_| Err(anyhow::anyhow!("{}", err_msg))).collect()
        }
    }
}

/// Shared helper for generator processors that support batch execution.
///
/// Passes (input, output) path pairs to `batch_fn`. On success, returns Ok for all products.
/// On failure, the batch error is returned for all products.
///
pub(crate) fn execute_generator_batch<F>(
    products: &[&Product],
    batch_fn: F,
) -> Vec<Result<()>>
where
    F: Fn(&[(&Path, &Path)]) -> Result<()>,
{
    let pairs: Vec<(&Path, &Path)> = products.iter()
        .map(|p| (p.primary_input(), p.primary_output()))
        .collect();

    match batch_fn(&pairs) {
        Ok(()) => products.iter().map(|_| Ok(())).collect(),
        Err(e) => {
            let err_msg = e.to_string();
            products.iter().map(|_| Err(anyhow::anyhow!("{}", err_msg))).collect()
        }
    }
}

pub(crate) use checkers::terms;
pub(crate) use generators::tags as tags_cmd;
pub use lua::LuaProcessor;

/// Map from processor name to processor instance. Used throughout the build pipeline.
pub type ProcessorMap = HashMap<String, Box<dyn Processor>>;

/// The type of processor - whether it generates new files, checks existing files,
/// or produces a mass of output files in a directory.
///
/// # Caching Behavior
///
/// All processor types use the cache to avoid redundant work:
///
/// - **Generators** produce output files (e.g., executables, rendered templates). The cache
///   stores copies of these outputs. On `rsconstruct clean`, output files are deleted but the cache
///   remains intact. On the next `rsconstruct build`, outputs are restored from cache (fast copy/hardlink)
///   instead of being regenerated.
///
/// - **Checkers** validate input files but produce no output files. The cache entry itself
///   serves as a "success marker". On `rsconstruct clean`, there's nothing to delete. On the next
///   `rsconstruct build`, if the cache entry exists and inputs haven't changed, the check is skipped
///   entirely (instant).
///
/// - **Creators** produce a mass of output files in a directory but don't enumerate
///   those outputs individually (e.g., pip → site-packages, npm → node_modules, cargo → target).
///   They use stamp files or empty outputs for cache tracking, similar to checkers.
///
/// This design ensures that `rsconstruct clean && rsconstruct build` is fast for all types - generators
/// restore from cache, checkers skip entirely, creators re-run only when inputs change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[derive(strum::EnumIter)]
pub enum ProcessorType {
    /// Generates new output files from input files (e.g., tera, cc_single_file).
    /// Products have non-empty `outputs` which are cached and can be restored.
    Generator,
    /// Checks/validates input files without producing output files (e.g., ruff, pylint, shellcheck).
    /// Products have empty `outputs`; the cache entry serves as the success marker.
    Checker,
    /// Runs a command and caches declared output files and directories
    /// (e.g., cargo, pip, npm, sphinx, mdbook, user-defined creators).
    Creator,
    /// Many inputs aggregated into (possibly) many output files and/or directories.
    /// Unlike Generator (one product per input file), creates a single product.
    Explicit,
    /// A user-defined processor implemented in Lua via the plugin runtime.
    /// Lua scripts may override this by declaring `processor_type()` returning
    /// "checker"/"generator"/"creator"/"explicit" — only scripts that omit that
    /// function are categorized as Lua.
    Lua,
}

impl ProcessorType {
    /// Returns the string representation
    pub fn as_str(&self) -> &'static str {
        match self {
            ProcessorType::Generator => "generator",
            ProcessorType::Checker => "checker",
            ProcessorType::Creator => "creator",
            ProcessorType::Explicit => "explicit",
            ProcessorType::Lua => "lua",
        }
    }

    /// Returns a human-readable description of this processor type.
    pub fn description(&self) -> &'static str {
        match self {
            ProcessorType::Generator => "Generates output files from input files (1 input -> 1 output per format)",
            ProcessorType::Checker => "Validates input files without producing outputs",
            ProcessorType::Creator => "Runs a command and caches declared output files and directories",
            ProcessorType::Explicit => "Many inputs aggregated into (possibly) many output files and/or directories",
            ProcessorType::Lua => "User-defined processor implemented in Lua via the plugin runtime",
        }
    }

}

/// Common base for all processors. Holds fields needed by boilerplate
/// Processor methods so each processor doesn't repeat them.
pub struct ProcessorBase {
    /// Human-readable description
    pub description: &'static str,
    /// Generator or Checker
    pub processor_type: ProcessorType,
}

impl ProcessorBase {
    pub fn generator(_name: &'static str, description: &'static str) -> Self {
        Self { description, processor_type: ProcessorType::Generator }
    }

    pub fn creator(_name: &'static str, description: &'static str) -> Self {
        Self { description, processor_type: ProcessorType::Creator }
    }

    pub fn checker(_name: &'static str, description: &'static str) -> Self {
        Self { description, processor_type: ProcessorType::Checker }
    }

    pub fn explicit(_name: &'static str, description: &'static str) -> Self {
        Self { description, processor_type: ProcessorType::Explicit }
    }

    pub fn description(&self) -> &str {
        self.description
    }

    pub fn processor_type(&self) -> ProcessorType {
        self.processor_type
    }

    pub fn config_json<C: Serialize>(config: &C) -> Option<String> {
        serde_json::to_string(config).ok()
    }

    pub fn clean(product: &Product, name: &str, verbose: bool) -> anyhow::Result<usize> {
        clean_outputs(product, name, verbose)
    }

    pub fn clean_output_dir(product: &Product, name: &str, verbose: bool) -> anyhow::Result<usize> {
        clean_output_dir(product, name, verbose)
    }
}

/// Trait for processors that can discover products for the build graph.
///
/// Processors come in three types (see [`ProcessorType`]):
/// - **Generators**: Create output files from inputs (must override `clean()`)
/// - **Checkers**: Validate inputs without producing outputs (use default `clean()`)
/// - **Creators**: Produce a mass of output files in a directory without enumerating them
///
/// # Implementing a Checker
///
/// Checkers are simpler - just implement the required methods and use defaults for the rest:
///
/// ```ignore
/// impl Processor for MyChecker {
///     fn description(&self) -> &str { "Check files with mytool" }
///     fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex, instance_name: &str) -> Result<()> {
///         discover_checker_products(graph, ..., instance_name)  // empty outputs
///     }
///     fn execute(&self, product: &Product) -> Result<()> {
///         run_mytool(product.primary_input())
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
/// impl Processor for MyGenerator {
///     fn description(&self) -> &str { "Generate files" }
///     fn processor_type(&self) -> ProcessorType { ProcessorType::Generator }
///     fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex, instance_name: &str) -> Result<()> {
///         graph.add_product(inputs, outputs, instance_name, ...)?;  // non-empty outputs
///     }
///     fn execute(&self, product: &Product) -> Result<()> { ... }
///     fn clean(&self, product: &Product, verbose: bool) -> Result<usize> {
///         clean_outputs(product, &product.processor, verbose)
///     }
///     fn auto_detect(&self, file_index: &FileIndex) -> bool { ... }
/// }
/// ```
///
/// Must be Sync + Send for parallel execution support.
pub trait Processor: Sync + Send {
    /// Human-readable description of what this processor does
    fn description(&self) -> &str;

    /// The type of this processor (generator or checker).
    /// Default is Checker since most processors are checkers.
    fn processor_type(&self) -> ProcessorType {
        ProcessorType::Checker
    }

    /// Access the scan configuration. Required for auto_detect and discover defaults.
    fn scan_config(&self) -> &crate::config::StandardConfig;

    /// Access the standard config fields. Override to enable defaults for
    /// config_json, max_jobs, supports_batch, and discover.
    fn standard_config(&self) -> Option<&crate::config::StandardConfig> {
        None
    }

    /// Discover all products this processor can produce.
    /// Default: standard checker discover using dep_inputs/dep_auto from standard_config.
    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex, instance_name: &str) -> Result<()> {
        let cfg = self.standard_config().expect("discover() requires standard_config() or must be overridden");
        checker_discover(graph, cfg, file_index, &cfg.dep_inputs, &cfg.dep_auto, cfg, instance_name)
    }

    /// Discover products for clean operation (outputs only, skip expensive dependency scanning).
    fn discover_for_clean(&self, graph: &mut BuildGraph, file_index: &FileIndex, instance_name: &str) -> Result<()> {
        self.discover(graph, file_index, instance_name)
    }

    /// Execute a single product
    fn execute(&self, product: &Product) -> Result<()>;

    /// Clean outputs for a product. Checkers: default does nothing. Generators: override.
    fn clean(&self, _product: &Product, _verbose: bool) -> Result<usize> {
        Ok(0)
    }

    /// Auto-detect whether this processor is relevant for the current project.
    /// Default: check if scan finds any files.
    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        checker_auto_detect(self.scan_config(), file_index)
    }

    /// Return the names of external tools required by this processor
    fn required_tools(&self) -> Vec<String> {
        Vec::new()
    }

    /// Return tool version commands: Vec of (tool_name, args_to_get_version).
    fn tool_version_commands(&self) -> Vec<(String, Vec<String>)> {
        self.required_tools()
            .into_iter()
            .map(|tool| (tool, vec!["--version".to_string()]))
            .collect()
    }

    /// Whether this processor is native (pure Rust, no external tools).
    fn is_native(&self) -> bool {
        false
    }

    /// Whether this processor supports real batch execution (passing multiple
    /// files to the tool in one invocation). Every processor must declare this
    /// explicitly — there is no default.
    fn supports_batch(&self) -> bool;

    /// Execute multiple products in one invocation.
    /// Only called when supports_batch() returns true.
    fn execute_batch(&self, products: &[&Product]) -> Vec<Result<()>> {
        products.iter().map(|p| self.execute(p)).collect()
    }

    /// Return the processor's configuration as JSON for config change detection.
    /// Default: serialize standard_config if available.
    fn config_json(&self) -> Option<String> {
        self.standard_config().and_then(|c| serde_json::to_string(c).ok())
    }

    /// Maximum concurrent jobs. Default: reads from standard_config().max_jobs.
    fn max_jobs(&self) -> Option<usize> {
        self.standard_config().and_then(|c| c.max_jobs)
    }
}

/// Central registry of all known external tools — single source of truth for
/// runtime category and install command. Both `tool_install_command()` and
/// `tool_runtime()` (in `builder/tools.rs`) look up data from this table.
///
/// Runtime categories: "python", "node", "ruby", "rust", "perl", "system"
///
/// A single way to install a tool.
pub struct InstallMethod {
    /// Package manager or method name (e.g., "pip", "apt", "npm", "snap", "cargo", "binary")
    pub method: &'static str,
    /// Package name for the package manager (e.g., "taplo-cli" for cargo, "texlive-latex-base" for apt)
    pub package: &'static str,
}

impl InstallMethod {
    /// Return the full install command (e.g., "pip install ruff", "sudo apt install -y shellcheck")
    pub fn command(&self) -> String {
        match self.method {
            "apt" => format!("sudo apt install -y {}", self.package),
            "snap" => format!("sudo snap install {}", self.package),
            "pip" => format!("pip install {}", self.package),
            "npm" => format!("npm install -g {}", self.package),
            "cargo" => format!("cargo install {}", self.package),
            "gem" => format!("gem install {}", self.package),
            _ => self.package.to_string(),
        }
    }

    /// Return the install command for multiple packages at once (batch install)
    pub fn batch_command(method: &str, packages: &[&str]) -> String {
        match method {
            "apt" => format!("sudo apt install -y {}", packages.join(" ")),
            "snap" => format!("sudo snap install {}", packages.join(" ")),
            "pip" => format!("pip install {}", packages.join(" ")),
            "npm" => format!("npm install -g {}", packages.join(" ")),
            "cargo" => format!("cargo install {}", packages.join(" ")),
            "gem" => format!("gem install {}", packages.join(" ")),
            _ => packages.iter().map(|p| p.to_string()).collect::<Vec<_>>().join("; "),
        }
    }
}

/// Information about an external tool: its name, runtime category, and install methods.
pub struct ToolInfo {
    /// Tool binary name
    pub name: &'static str,
    /// Runtime category ("python", "node", "ruby", "rust", "perl", "system")
    pub runtime: &'static str,
    /// Install methods, ordered by preference (first is the default)
    pub install_methods: &'static [InstallMethod],
}

pub static TOOLS: &[ToolInfo] = &[
    // Python tools
    ToolInfo { name: "ruff", runtime: "python", install_methods: &[InstallMethod { method: "pip", package: "ruff" }] },
    ToolInfo { name: "pylint", runtime: "python", install_methods: &[InstallMethod { method: "pip", package: "pylint" }] },
    ToolInfo { name: "mypy", runtime: "python", install_methods: &[InstallMethod { method: "pip", package: "mypy" }] },
    ToolInfo { name: "pyrefly", runtime: "python", install_methods: &[InstallMethod { method: "pip", package: "pyrefly" }] },
    ToolInfo { name: "yamllint", runtime: "python", install_methods: &[InstallMethod { method: "pip", package: "yamllint" }] },
    ToolInfo { name: "sphinx-build", runtime: "python", install_methods: &[InstallMethod { method: "pip", package: "sphinx" }] },
    ToolInfo { name: "pip", runtime: "python", install_methods: &[InstallMethod { method: "system", package: "python3 -m ensurepip" }] },
    ToolInfo { name: "jsonlint", runtime: "python", install_methods: &[InstallMethod { method: "pip", package: "demjson3" }] },
    ToolInfo { name: "cpplint", runtime: "python", install_methods: &[InstallMethod { method: "pip", package: "cpplint" }] },
    ToolInfo { name: "black", runtime: "python", install_methods: &[InstallMethod { method: "pip", package: "black" }] },
    ToolInfo { name: "pytest", runtime: "python", install_methods: &[InstallMethod { method: "pip", package: "pytest" }] },
    ToolInfo { name: "a2x", runtime: "python", install_methods: &[InstallMethod { method: "apt", package: "asciidoc" }] },
    ToolInfo { name: "python3", runtime: "python", install_methods: &[InstallMethod { method: "apt", package: "python3" }] },
    // Node tools
    ToolInfo { name: "marp", runtime: "node", install_methods: &[InstallMethod { method: "npm", package: "@marp-team/marp-cli" }] },
    ToolInfo { name: "mmdc", runtime: "node", install_methods: &[InstallMethod { method: "npm", package: "@mermaid-js/mermaid-cli" }] },
    ToolInfo { name: "node_modules/.bin/markdownlint", runtime: "node", install_methods: &[InstallMethod { method: "npm", package: "markdownlint-cli" }] },
    ToolInfo { name: "markdownlint", runtime: "node", install_methods: &[InstallMethod { method: "npm", package: "markdownlint-cli" }] },
    ToolInfo { name: "eslint", runtime: "node", install_methods: &[InstallMethod { method: "npm", package: "eslint" }] },
    ToolInfo { name: "htmlhint", runtime: "node", install_methods: &[InstallMethod { method: "npm", package: "htmlhint" }] },
    ToolInfo { name: "jshint", runtime: "node", install_methods: &[InstallMethod { method: "npm", package: "jshint" }] },
    ToolInfo { name: "npm", runtime: "node", install_methods: &[InstallMethod { method: "apt", package: "npm" }] },
    ToolInfo { name: "node", runtime: "node", install_methods: &[InstallMethod { method: "apt", package: "nodejs" }] },
    // Ruby tools
    ToolInfo { name: "gems/bin/mdl", runtime: "ruby", install_methods: &[InstallMethod { method: "gem", package: "mdl" }] },
    ToolInfo { name: "mdl", runtime: "ruby", install_methods: &[InstallMethod { method: "gem", package: "mdl" }] },
    ToolInfo { name: "bundle", runtime: "ruby", install_methods: &[InstallMethod { method: "gem", package: "bundler" }] },
    ToolInfo { name: "ruby", runtime: "ruby", install_methods: &[InstallMethod { method: "apt", package: "ruby" }] },
    // Rust tools
    ToolInfo { name: "mdbook", runtime: "rust", install_methods: &[InstallMethod { method: "cargo", package: "mdbook" }] },
    ToolInfo { name: "rumdl", runtime: "rust", install_methods: &[InstallMethod { method: "cargo", package: "rumdl" }] },
    ToolInfo { name: "taplo", runtime: "rust", install_methods: &[
        InstallMethod { method: "binary", package: "curl -fsSL https://github.com/tamasfe/taplo/releases/latest/download/taplo-linux-x86_64.gz | gunzip > /tmp/taplo && chmod +x /tmp/taplo && sudo mv /tmp/taplo /usr/local/bin/taplo" },
        InstallMethod { method: "cargo", package: "taplo-cli" },
    ]},
    ToolInfo { name: "cargo", runtime: "rust", install_methods: &[InstallMethod { method: "binary", package: "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh" }] },
    ToolInfo { name: "rustc", runtime: "rust", install_methods: &[InstallMethod { method: "binary", package: "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh" }] },
    // Perl tools
    ToolInfo { name: "perl", runtime: "perl", install_methods: &[InstallMethod { method: "apt", package: "perl" }] },
    ToolInfo { name: "markdown", runtime: "perl", install_methods: &[InstallMethod { method: "apt", package: "markdown" }] },
    ToolInfo { name: "checkpatch.pl", runtime: "perl", install_methods: &[InstallMethod { method: "manual", package: "install from Linux kernel source: scripts/checkpatch.pl" }] },
    // System tools
    ToolInfo { name: "shellcheck", runtime: "system", install_methods: &[InstallMethod { method: "apt", package: "shellcheck" }] },
    ToolInfo { name: "luacheck", runtime: "system", install_methods: &[InstallMethod { method: "apt", package: "lua-check" }] },
    ToolInfo { name: "cppcheck", runtime: "system", install_methods: &[InstallMethod { method: "apt", package: "cppcheck" }] },
    ToolInfo { name: "clang-tidy", runtime: "system", install_methods: &[InstallMethod { method: "apt", package: "clang-tidy" }] },
    ToolInfo { name: "gcc", runtime: "system", install_methods: &[InstallMethod { method: "apt", package: "gcc" }] },
    ToolInfo { name: "g++", runtime: "system", install_methods: &[InstallMethod { method: "apt", package: "g++" }] },
    ToolInfo { name: "clang", runtime: "system", install_methods: &[InstallMethod { method: "apt", package: "clang" }] },
    ToolInfo { name: "clang++", runtime: "system", install_methods: &[InstallMethod { method: "apt", package: "clang" }] },
    ToolInfo { name: "ar", runtime: "system", install_methods: &[InstallMethod { method: "apt", package: "binutils" }] },
    ToolInfo { name: "make", runtime: "system", install_methods: &[InstallMethod { method: "apt", package: "make" }] },
    ToolInfo { name: "jq", runtime: "system", install_methods: &[InstallMethod { method: "apt", package: "jq" }] },
    ToolInfo { name: "aspell", runtime: "system", install_methods: &[InstallMethod { method: "apt", package: "aspell" }] },
    ToolInfo { name: "pandoc", runtime: "system", install_methods: &[InstallMethod { method: "apt", package: "pandoc" }] },
    ToolInfo { name: "pdflatex", runtime: "system", install_methods: &[InstallMethod { method: "apt", package: "texlive-latex-base" }] },
    ToolInfo { name: "qpdf", runtime: "system", install_methods: &[InstallMethod { method: "apt", package: "qpdf" }] },
    ToolInfo { name: "dot", runtime: "system", install_methods: &[InstallMethod { method: "apt", package: "graphviz" }] },
    ToolInfo { name: "drawio", runtime: "system", install_methods: &[InstallMethod { method: "snap", package: "drawio" }] },
    ToolInfo { name: "libreoffice", runtime: "system", install_methods: &[InstallMethod { method: "apt", package: "libreoffice" }] },
    ToolInfo { name: "flock", runtime: "system", install_methods: &[InstallMethod { method: "apt", package: "util-linux" }] },
    ToolInfo { name: "pdfunite", runtime: "system", install_methods: &[InstallMethod { method: "apt", package: "poppler-utils" }] },
    ToolInfo { name: "google-chrome", runtime: "system", install_methods: &[InstallMethod { method: "apt", package: "google-chrome-stable" }] },
    ToolInfo { name: "objdump", runtime: "system", install_methods: &[InstallMethod { method: "apt", package: "binutils" }] },
    ToolInfo { name: "tidy", runtime: "system", install_methods: &[InstallMethod { method: "apt", package: "tidy" }] },
    ToolInfo { name: "xmllint", runtime: "system", install_methods: &[InstallMethod { method: "apt", package: "libxml2-utils" }] },
    ToolInfo { name: "svglint", runtime: "node", install_methods: &[InstallMethod { method: "npm", package: "svglint" }] },
    ToolInfo { name: "svgo", runtime: "node", install_methods: &[InstallMethod { method: "npm", package: "svgo" }] },
    ToolInfo { name: "cmake", runtime: "system", install_methods: &[InstallMethod { method: "apt", package: "cmake" }] },
    ToolInfo { name: "protoc", runtime: "system", install_methods: &[InstallMethod { method: "apt", package: "protobuf-compiler" }] },
    ToolInfo { name: "sass", runtime: "node", install_methods: &[InstallMethod { method: "npm", package: "sass" }] },
    ToolInfo { name: "hadolint", runtime: "system", install_methods: &[
        InstallMethod { method: "binary", package: "wget -O ~/.local/bin/hadolint https://github.com/hadolint/hadolint/releases/latest/download/hadolint-Linux-x86_64 && chmod +x ~/.local/bin/hadolint" },
        InstallMethod { method: "brew", package: "hadolint" },
        InstallMethod { method: "nix", package: "hadolint" },
        InstallMethod { method: "apt", package: "hadolint" },
    ]},
    ToolInfo { name: "php", runtime: "system", install_methods: &[InstallMethod { method: "apt", package: "php-cli" }] },
    ToolInfo { name: "checkstyle", runtime: "system", install_methods: &[InstallMethod { method: "apt", package: "checkstyle" }] },
    ToolInfo { name: "yq", runtime: "system", install_methods: &[
        InstallMethod { method: "pip", package: "yq" },
        InstallMethod { method: "snap", package: "yq" },
        InstallMethod { method: "apt", package: "yq" },
    ]},
    // Node tools (additional)
    ToolInfo { name: "stylelint", runtime: "node", install_methods: &[InstallMethod { method: "npm", package: "stylelint" }] },
    ToolInfo { name: "jslint", runtime: "node", install_methods: &[InstallMethod { method: "npm", package: "jslint" }] },
    ToolInfo { name: "standard", runtime: "node", install_methods: &[InstallMethod { method: "npm", package: "standard" }] },
    ToolInfo { name: "htmllint", runtime: "node", install_methods: &[InstallMethod { method: "npm", package: "htmllint-cli" }] },
    ToolInfo { name: "slidev", runtime: "node", install_methods: &[InstallMethod { method: "npm", package: "@slidev/cli" }] },
    // Perl tools (additional)
    ToolInfo { name: "perlcritic", runtime: "perl", install_methods: &[InstallMethod { method: "apt", package: "libperl-critic-perl" }] },
    // Ruby tools (additional)
    ToolInfo { name: "jekyll", runtime: "ruby", install_methods: &[InstallMethod { method: "gem", package: "jekyll" }] },
    // Built-in / coreutils
    ToolInfo { name: "true", runtime: "system", install_methods: &[InstallMethod { method: "system", package: "coreutils" }] },
];

/// Look up a tool by name.
pub fn tool_info(tool: &str) -> Option<&'static ToolInfo> {
    TOOLS.iter().find(|t| t.name == tool)
}

/// Return the default install command for a tool, if known.
pub fn tool_install_command(tool: &str) -> Option<String> {
    tool_info(tool).and_then(|t| t.install_methods.first().map(|m| m.command()))
}

/// Return the runtime category for a tool, if known.
pub fn tool_runtime(tool: &str) -> Option<&'static str> {
    tool_info(tool).map(|t| t.runtime)
}

/// Timing for a single product execution
#[derive(Debug, Clone, PartialEq)]
pub struct ProductTiming {
    pub display: String,
    pub processor: String,
    pub duration: Duration,
    /// Offset from the build start time (for trace output)
    pub start_offset: Option<Duration>,
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

/// A single failed product with structured error info for `rsconstruct edit`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FailedProduct {
    /// Primary input file path
    pub file: String,
    /// Processor (instance) name
    pub processor: String,
    /// Error message from the tool
    pub error: String,
}

/// Aggregated statistics from all processors
#[derive(Default)]
pub struct BuildStats {
    pub categories: Vec<ProcessStats>,
    pub total_duration: Duration,
    pub failed_count: usize,
    pub failed_messages: Vec<String>,
    /// Structured failure details for `rsconstruct edit`
    pub failed_details: Vec<FailedProduct>,
    pub phase_timings: Vec<(String, Duration)>,
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

            let total_flaky = self.total_flaky();
            // Always show every category, including zero counts, so the line
            // shape is identical across builds and easy to scan/grep. Work
            // done (built, restored, failed) leads the line; idle counts
            // (unchanged, flaky) go in parentheses.
            let built_part = if total_files_created > 0 {
                format!("{} built ({} files created)", total_processed, total_files_created)
            } else {
                format!("{} built", total_processed)
            };
            let restored_part = if total_files_restored > 0 {
                format!("{} restored ({} files)", total_restored, total_files_restored)
            } else {
                format!("{} restored", total_restored)
            };
            let lead = format!(
                "{}, {}, {} failed",
                built_part, restored_part, total_failed,
            );
            let aside = format!("{} unchanged, {} flaky", total_skipped, total_flaky);

            // Emitted without color: the final "Exited with ..." line printed
            // by main() is the one coloured green/red so there's a single
            // signal of overall success/failure.
            println!("[build] summary: {} ({})", lead, aside);
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

            // Phase timings
            if !self.phase_timings.is_empty() {
                let mut builder = tabled::builder::Builder::new();
                builder.push_record(["Phase", "Duration"]);
                for (name, dur) in &self.phase_timings {
                    builder.push_record([name.to_string(), format!("{:.3}s", dur.as_secs_f64())]);
                }
                crate::color::print_table(builder.build());
            }

            // Per-product timings
            for cat in &self.categories {
                for pt in &cat.product_timings {
                    println!("[{}] {} {}", pt.processor, pt.display,
                        color::dim(&format!("({:.3}s)", pt.duration.as_secs_f64())));
                }
            }

            let total: f64 = self.phase_timings.iter().map(|(_, d)| d.as_secs_f64()).sum();
            println!("{}", color::bold(&format!("Total: {:.3}s", total)));
        }
    }
}

// ----------------------------------------------------------------------------
// Shared runtime types for data-driven per-processor files.
//
// Most single-file processors don't need their own Processor struct — they
// configure one of these generic runtimes instead and submit a plugin entry.
// Moved here from checkers/simple.rs and generators/simple.rs so the
// checkers/ and generators/ directories contain ONLY per-processor files.
// ----------------------------------------------------------------------------

/// A simple checker processor driven entirely by data.
/// Each trivial checker file (ruff.rs, pylint.rs, etc.) registers an instance
/// of this struct with its own `SimpleCheckerParams`.
pub struct SimpleChecker {
    config: CheckerConfigWithCommand,
    params: SimpleCheckerParams,
}

impl SimpleChecker {
    pub fn new(config: CheckerConfigWithCommand, params: SimpleCheckerParams) -> Self {
        Self { config, params }
    }

    fn check_files(&self, files: &[&Path]) -> Result<()> {
        let tool = self.config.standard.require_command(self.params.description)?;
        if self.params.prepend_args.is_empty() {
            run_checker(tool, self.params.subcommand, &self.config.standard.args, files)
        } else {
            let mut combined_args: Vec<String> = self.params.prepend_args.iter().map(|s| s.to_string()).collect();
            combined_args.extend_from_slice(&self.config.standard.args);
            run_checker(tool, self.params.subcommand, &combined_args, files)
        }
    }
}

impl Processor for SimpleChecker {
    fn scan_config(&self) -> &StandardConfig {
        &self.config.standard
    }

    fn standard_config(&self) -> Option<&StandardConfig> {
        Some(&self.config.standard)
    }

    fn description(&self) -> &str {
        self.params.description
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        !file_index.scan(&self.config.standard, true).is_empty()
    }

    fn required_tools(&self) -> Vec<String> {
        let mut tools = vec![self.config.standard.command.clone()];
        for t in self.params.extra_tools {
            tools.push(t.to_string());
        }
        tools
    }

    fn discover(
        &self,
        graph: &mut BuildGraph,
        file_index: &FileIndex,
        instance_name: &str,
    ) -> Result<()> {
        let mut dep_inputs = self.config.standard.dep_inputs.clone();
        for ai in &self.config.standard.dep_auto {
            dep_inputs.extend(config_file_inputs(ai));
        }
        discover_checker_products(
            graph, &self.config.standard, file_index, &dep_inputs, &self.config, instance_name,
        )
    }

    fn execute(&self, product: &Product) -> Result<()> {
        self.check_files(&[product.primary_input()])
    }

    fn supports_batch(&self) -> bool {
        self.config.standard.batch
    }

    fn execute_batch(&self, products: &[&Product]) -> Vec<Result<()>> {
        execute_checker_batch(products, |files| self.check_files(files))
    }
}

/// How a simple generator discovers its products.
#[derive(Copy, Clone)]
pub(crate) enum DiscoverMode {
    /// Discover one product per source x format (uses config.formats).
    MultiFormat,
    /// Discover one product per source file with a fixed output extension.
    SingleFormat(&'static str),
}

/// Parameters for a [`SimpleGenerator`]. Each trivial generator file
/// (mermaid.rs, pandoc.rs, etc.) configures one and registers it via the
/// processor registry.
#[derive(Copy, Clone)]
pub(crate) struct SimpleGeneratorParams {
    pub description: &'static str,
    pub extra_tools: &'static [&'static str],
    pub discover_mode: DiscoverMode,
    pub execute_fn: fn(&StandardConfig, &Product) -> Result<()>,
    pub is_native: bool,
}

/// Data-driven generator processor. Replaces identical boilerplate across
/// generators that use `StandardConfig` with standard discover logic.
pub struct SimpleGenerator {
    base: ProcessorBase,
    config: StandardConfig,
    params: SimpleGeneratorParams,
}

impl SimpleGenerator {
    pub fn new(config: StandardConfig, params: SimpleGeneratorParams) -> Self {
        Self {
            base: ProcessorBase::generator("", params.description),
            config,
            params,
        }
    }
}

impl Processor for SimpleGenerator {
    fn scan_config(&self) -> &StandardConfig {
        &self.config
    }

    fn standard_config(&self) -> Option<&StandardConfig> {
        Some(&self.config)
    }

    fn description(&self) -> &str {
        self.base.description()
    }

    fn processor_type(&self) -> ProcessorType {
        self.base.processor_type()
    }

    fn config_json(&self) -> Option<String> {
        ProcessorBase::config_json(&self.config)
    }

    fn clean(&self, product: &Product, verbose: bool) -> Result<usize> {
        ProcessorBase::clean(product, &product.processor, verbose)
    }

    fn is_native(&self) -> bool {
        self.params.is_native
    }

    fn required_tools(&self) -> Vec<String> {
        if self.params.is_native {
            self.params.extra_tools.iter().map(|t| t.to_string()).collect()
        } else {
            let mut tools = vec![self.config.command.clone()];
            for t in self.params.extra_tools {
                tools.push(t.to_string());
            }
            tools
        }
    }

    fn max_jobs(&self) -> Option<usize> {
        self.config.max_jobs
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex, instance_name: &str) -> Result<()> {
        let params = generators::DiscoverParams {
            scan: &self.config,
            dep_inputs: &self.config.dep_inputs,
            config: &self.config,
            output_dir: &self.config.output_dir,
            processor_name: instance_name,
        };
        match &self.params.discover_mode {
            DiscoverMode::MultiFormat => {
                generators::discover_multi_format(graph, file_index, &params, &self.config.formats)
            }
            DiscoverMode::SingleFormat(ext) => {
                generators::discover_single_format(graph, file_index, &params, ext)
            }
        }
    }

    fn supports_batch(&self) -> bool { false }

    fn execute(&self, product: &Product) -> Result<()> {
        (self.params.execute_fn)(&self.config, product)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::create_all_default_processors;

    /// Verify that every tool declared by any processor's `required_tools()` has
    /// an entry in the central TOOLS registry (install command + runtime category).
    /// This prevents silent gaps like the missing `ar` tool.
    #[test]
    fn all_required_tools_have_registry_entries() {
        let processors = create_all_default_processors();
        for (proc_name, proc) in &processors {
            for tool in proc.required_tools() {
                if tool.is_empty() {
                    continue;
                }
                assert!(
                    tool_install_command(&tool).is_some(),
                    "Processor '{}' requires tool '{}' which has no install command in TOOLS",
                    proc_name, tool
                );
                assert!(
                    tool_runtime(&tool).is_some(),
                    "Processor '{}' requires tool '{}' which has no runtime category in TOOLS",
                    proc_name, tool
                );
            }
        }
    }
}
