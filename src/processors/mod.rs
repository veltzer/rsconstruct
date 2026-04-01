mod checkers;
mod generators;
mod mass_generators;
pub mod lua_processor;
pub(crate) mod word_manager;

use anyhow::{Context, Result};
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
use crate::config::{output_config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};

/// Processor name constants — auto-generated from the central registry in `src/registry.rs`.
macro_rules! gen_processor_names {
    ( $( $const_name:ident, $field:ident, $config_type:ty, $proc_type:ty,
         ($($scan_args:tt)*); )* ) => {
        pub mod names {
            $( pub const $const_name: &str = stringify!($field); )*
        }
    };
}
for_each_processor!(gen_processor_names);

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
                let output = result.context("Failed to wait for child process")?;
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

/// Compute the scan root directory from a ScanConfig.
/// Returns empty path if scan_dir is empty, otherwise the scan_dir as a relative path.
pub(crate) fn scan_root(scan: &crate::config::ScanConfig) -> PathBuf {
    PathBuf::from(scan.scan_dir())
}

/// Check if a scan root is valid (empty means current dir, otherwise must exist).
pub(crate) fn scan_root_valid(scan: &crate::config::ScanConfig) -> bool {
    let root = scan_root(scan);
    root.as_os_str().is_empty() || root.exists()
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
    child.stdin.take().expect(crate::errors::STDIN_PIPED).write_all(dot_content.as_bytes())?;
    let output = child.wait_with_output()?;
    check_command_output(&output, "dot")?;
    Ok(String::from_utf8(output.stdout)?)
}

/// Append new words to a words file without truncating existing content.
/// Used by aspell and spellcheck processors for their auto_add_words feature.
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

/// Remove the output_dir of a product. Used by mass generator clean() methods.
/// Returns 1 if the directory was removed, 0 otherwise.
pub(crate) fn clean_output_dir(product: &Product, processor_name: &str, verbose: bool) -> Result<usize> {
    if let Some(ref output_dir) = product.output_dir
        && output_dir.exists()
    {
        if verbose {
            println!("Removing {} output directory: {}", processor_name, output_dir.display());
        }
        fs::remove_dir_all(output_dir.as_ref())?;
        return Ok(1);
    }
    Ok(0)
}

/// Build the input list for mass generators: anchor first, then sibling files
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

/// Combine the scan_root_valid check, scan, and empty check that mass generators
/// repeat in their discover() methods. Returns None if the scan root is invalid
/// or no files were found, otherwise returns the list of files.
pub(crate) fn scan_or_skip(scan: &crate::config::ScanConfig, file_index: &FileIndex) -> Option<Vec<PathBuf>> {
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
    pub scan: &'a crate::config::ScanConfig,
    pub file_index: &'a FileIndex,
    pub extra_inputs: &'a [String],
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
/// `anchor_parent/dir_name`, enabling directory-level caching for mass generators.
pub(crate) fn discover_directory_products(
    graph: &mut BuildGraph,
    opts: DirectoryProductOpts<'_, impl serde::Serialize>,
) -> Result<()> {
    let DirectoryProductOpts { scan, file_index, extra_inputs, cfg_hash, siblings, processor_name, output_dir_name } = opts;
    let files = file_index.scan(scan, true);
    if files.is_empty() {
        return Ok(());
    }

    let hash = Some(output_config_hash(cfg_hash, &[]));
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
    let hash = Some(output_config_hash(cfg_hash, &[]));
    let extra = resolve_extra_inputs(extra_inputs)?;
    for file in files {
        let mut inputs = Vec::with_capacity(1 + extra.len());
        inputs.push(file);
        inputs.extend_from_slice(&extra);
        // Empty outputs: cache entry = success record
        graph.add_product(inputs, vec![], processor_name, hash.clone())?;
    }
    Ok(())
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

// Re-export from subdirectories
pub use checkers::{
    AsciiCheckProcessor, AspellProcessor,
    CheckpatchProcessor, ClippyProcessor, ClangTidyProcessor, CppcheckProcessor, CpplintProcessor,
    CheckstyleProcessor, CmakeProcessor,
    HadolintProcessor, EslintProcessor, HtmlhintProcessor, HtmllintProcessor,
    JekyllProcessor, JshintProcessor, JslintProcessor, JqProcessor, JsonlintProcessor, JsonSchemaProcessor,
    LuacheckProcessor, MakeProcessor, MarkdownlintProcessor, MdlProcessor, MypyProcessor,
    PerlcriticProcessor, PhpLintProcessor, PylintProcessor, PyreflyProcessor, RuffProcessor, RumdlProcessor,
    ScriptCheckProcessor, ShellcheckProcessor, SlidevProcessor, SpellcheckProcessor,
    StandardProcessor, StylelintProcessor,
    TaploProcessor, TermsProcessor, TidyProcessor, XmllintProcessor, YamllintProcessor, YqProcessor,
};
pub use generators::{A2xProcessor, CcSingleFileProcessor, ChromiumProcessor, DrawioProcessor, LibreofficeProcessor, LinuxModuleProcessor, MakoProcessor, MarpProcessor, MarkdownProcessor, MermaidProcessor, ObjdumpProcessor, PandocProcessor, PdflatexProcessor, PdfuniteProcessor, TagsProcessor, TeraProcessor};
pub use mass_generators::{CargoProcessor, CcProcessor, GemProcessor, MdbookProcessor, NpmProcessor, PipProcessor, SphinxProcessor};
pub(crate) use generators::tags as tags_cmd;
pub(crate) use checkers::terms;
pub use lua_processor::LuaProcessor;

/// Map from processor name to processor instance. Used throughout the build pipeline.
pub type ProcessorMap = HashMap<String, Box<dyn ProductDiscovery>>;

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
/// - **Mass generators** produce a mass of output files in a directory but don't enumerate
///   those outputs individually (e.g., pip → site-packages, npm → node_modules, cargo → target).
///   They use stamp files or empty outputs for cache tracking, similar to checkers.
///
/// This design ensures that `rsconstruct clean && rsconstruct build` is fast for all types - generators
/// restore from cache, checkers skip entirely, mass generators re-run only when inputs change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessorType {
    /// Generates new output files from input files (e.g., tera, cc_single_file).
    /// Products have non-empty `outputs` which are cached and can be restored.
    Generator,
    /// Checks/validates input files without producing output files (e.g., ruff, pylint, shellcheck).
    /// Products have empty `outputs`; the cache entry serves as the success marker.
    Checker,
    /// Produces a mass of output files in a directory without enumerating them individually
    /// (e.g., pip → site-packages, npm → node_modules, cargo → target).
    /// May use stamp files for cache tracking.
    MassGenerator,
}

impl ProcessorType {
    /// Returns the string representation
    pub fn as_str(&self) -> &'static str {
        match self {
            ProcessorType::Generator => "generator",
            ProcessorType::Checker => "checker",
            ProcessorType::MassGenerator => "mass_generator",
        }
    }
}

/// Trait for processors that can discover products for the build graph.
///
/// Processors come in three types (see [`ProcessorType`]):
/// - **Generators**: Create output files from inputs (must override `clean()`)
/// - **Checkers**: Validate inputs without producing outputs (use default `clean()`)
/// - **Mass generators**: Produce a mass of output files in a directory without enumerating them
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
/// impl ProductDiscovery for MyGenerator {
///     fn description(&self) -> &str { "Generate files" }
///     fn processor_type(&self) -> ProcessorType { ProcessorType::Generator }
///     fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex) -> Result<()> {
///         graph.add_product(inputs, outputs, "mygen", ...)?;  // non-empty outputs
///     }
///     fn execute(&self, product: &Product) -> Result<()> { ... }
///     fn clean(&self, product: &Product, verbose: bool) -> Result<usize> {
///         clean_outputs(product, "mygen", verbose)
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

    /// Clean outputs for a product (called by `rsconstruct clean`).
    /// When `verbose` is true, prints per-file removal messages.
    /// Returns the number of files removed.
    ///
    /// - **Checkers**: Use the default (do nothing) - checkers have no output files.
    ///   The cache entry remains intact, so the next build will skip the check.
    ///
    /// - **Generators**: Must override to delete output files. Use `clean_outputs()`
    ///   helper. The cache entry remains intact, so the next build will restore
    ///   outputs from cache instead of regenerating them.
    fn clean(&self, _product: &Product, _verbose: bool) -> Result<usize> {
        Ok(0)
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

/// Helper to define a tool with a single install method.
/// If package name differs from tool name, use: `tool!("bin", "runtime", "method", "package")`
/// If package name equals tool name: `tool!("name", "runtime", "method")`
macro_rules! tool {
    ($name:expr, $runtime:expr, $method:expr, $package:expr) => {
        ToolInfo {
            name: $name,
            runtime: $runtime,
            install_methods: &[InstallMethod { method: $method, package: $package }],
        }
    };
    ($name:expr, $runtime:expr, $method:expr) => {
        ToolInfo {
            name: $name,
            runtime: $runtime,
            install_methods: &[InstallMethod { method: $method, package: $name }],
        }
    };
}

/// Helper to define a tool with multiple install methods (first is default).
macro_rules! tool_multi {
    ($name:expr, $runtime:expr, $( ($method:expr, $package:expr) ),+ $(,)?) => {
        ToolInfo {
            name: $name,
            runtime: $runtime,
            install_methods: &[ $( InstallMethod { method: $method, package: $package } ),+ ],
        }
    };
}

pub static TOOLS: &[ToolInfo] = &[
    // Python tools
    tool!("ruff",            "python", "pip"),
    tool!("pylint",          "python", "pip"),
    tool!("mypy",            "python", "pip"),
    tool!("pyrefly",         "python", "pip"),
    tool!("yamllint",        "python", "pip"),
    tool!("sphinx-build",    "python", "pip", "sphinx"),
    tool!("pip",             "python", "system", "python3 -m ensurepip"),
    tool!("jsonlint",        "python", "pip"),
    tool!("cpplint",         "python", "pip"),
    tool!("a2x",             "python", "apt", "asciidoc"),
    tool!("python3",         "python", "apt"),
    // Node tools
    tool!("marp",            "node", "npm", "@marp-team/marp-cli"),
    tool!("mmdc",            "node", "npm", "@mermaid-js/mermaid-cli"),
    tool!("node_modules/.bin/markdownlint", "node", "npm", "markdownlint-cli"),
    tool!("markdownlint",    "node", "npm", "markdownlint-cli"),
    tool!("eslint",          "node", "npm"),
    tool!("htmlhint",        "node", "npm"),
    tool!("jshint",          "node", "npm"),
    tool!("npm",             "node", "apt"),
    tool!("node",            "node", "apt", "nodejs"),
    // Ruby tools
    tool!("gems/bin/mdl",    "ruby", "gem", "mdl"),
    tool!("mdl",             "ruby", "gem"),
    tool!("bundle",          "ruby", "gem", "bundler"),
    tool!("ruby",            "ruby", "apt"),
    // Rust tools
    tool!("mdbook",          "rust", "cargo"),
    tool!("rumdl",           "rust", "cargo"),
    tool!("taplo",           "rust", "cargo", "taplo-cli"),
    tool!("cargo",           "rust", "binary", "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"),
    tool!("rustc",           "rust", "binary", "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"),
    // Perl tools
    tool!("perl",            "perl", "apt"),
    tool!("markdown",        "perl", "apt"),
    tool!("checkpatch.pl",   "perl", "manual", "install from Linux kernel source: scripts/checkpatch.pl"),
    // System tools
    tool!("shellcheck",      "system", "apt"),
    tool!("luacheck",        "system", "apt", "lua-check"),
    tool!("cppcheck",        "system", "apt"),
    tool!("clang-tidy",      "system", "apt"),
    tool!("gcc",             "system", "apt"),
    tool!("g++",             "system", "apt"),
    tool!("clang",           "system", "apt"),
    tool!("clang++",         "system", "apt", "clang"),
    tool!("ar",              "system", "apt", "binutils"),
    tool!("make",            "system", "apt"),
    tool!("jq",              "system", "apt"),
    tool!("aspell",          "system", "apt"),
    tool!("pandoc",          "system", "apt"),
    tool!("pdflatex",        "system", "apt", "texlive-latex-base"),
    tool!("qpdf",            "system", "apt"),
    tool!("dot",             "system", "apt", "graphviz"),
    tool!("drawio",          "system", "snap"),
    tool!("libreoffice",     "system", "apt"),
    tool!("flock",           "system", "apt", "util-linux"),
    tool!("pdfunite",        "system", "apt", "poppler-utils"),
    tool!("google-chrome",   "system", "apt", "google-chrome-stable"),
    tool!("objdump",         "system", "apt", "binutils"),
    tool!("tidy",            "system", "apt"),
    tool!("xmllint",         "system", "apt", "libxml2-utils"),
    tool!("cmake",           "system", "apt"),
    tool_multi!("hadolint",   "system",
        ("binary", "wget -O ~/.local/bin/hadolint https://github.com/hadolint/hadolint/releases/latest/download/hadolint-Linux-x86_64 && chmod +x ~/.local/bin/hadolint"),
        ("brew",   "hadolint"),
        ("nix",    "hadolint"),
        ("apt",    "hadolint"),
    ),
    tool!("php",             "system", "apt", "php-cli"),
    tool!("checkstyle",      "system", "apt"),
    tool_multi!("yq",        "system",
        ("pip",  "yq"),
        ("snap", "yq"),
        ("apt",  "yq"),
    ),
    // Node tools (additional)
    tool!("stylelint",       "node", "npm"),
    tool!("jslint",          "node", "npm"),
    tool!("standard",        "node", "npm"),
    tool!("htmllint",        "node", "npm", "htmllint-cli"),
    tool!("slidev",          "node", "npm", "@slidev/cli"),
    // Perl tools (additional)
    tool!("perlcritic",      "perl", "apt", "libperl-critic-perl"),
    // Ruby tools (additional)
    tool!("jekyll",          "ruby", "gem"),
    // Built-in / coreutils
    tool!("true",            "system", "system", "coreutils"),
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

            // Phase timings
            if !self.phase_timings.is_empty() {
                let max_name_len = self.phase_timings.iter().map(|(n, _)| n.len()).max().unwrap_or(0);
                for (name, dur) in &self.phase_timings {
                    println!("  {:width$} {:.3}s", name, dur.as_secs_f64(), width = max_name_len);
                }
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
