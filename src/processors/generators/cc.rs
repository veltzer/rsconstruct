use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use regex::Regex;

use crate::config::{CcConfig, IncludeScanner, config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProductDiscovery, scan_root, clean_outputs, format_command, run_command};

/// Global cache for shell command results to avoid running the same command multiple times.
/// Key is the command string, value is the resulting flags.
static SHELL_COMMAND_CACHE: Mutex<Option<HashMap<String, Vec<String>>>> = Mutex::new(None);

/// Get or compute flags from a shell command, caching the result.
fn cached_shell_command(cmd_line: &str, runner: impl FnOnce(&str) -> Result<Vec<String>>) -> Result<Vec<String>> {
    let mut cache = SHELL_COMMAND_CACHE.lock().unwrap();
    let cache = cache.get_or_insert_with(HashMap::new);

    if let Some(cached) = cache.get(cmd_line) {
        return Ok(cached.clone());
    }

    let result = runner(cmd_line)?;
    cache.insert(cmd_line.to_string(), result.clone());
    Ok(result)
}

/// Per-file compile/link flags extracted from source comments.
#[derive(Default)]
struct SourceFlags {
    compile_args_before: Vec<String>,
    compile_args_after: Vec<String>,
    link_args_before: Vec<String>,
    link_args_after: Vec<String>,
}

/// Parse per-file flags from C/C++ source comment lines.
///
/// Supported comment formats:
///   // EXTRA_COMPILE_FLAGS_BEFORE=-pthread -I/usr/local/include
///   /* EXTRA_COMPILE_FLAGS_AFTER=-O2 -DNDEBUG */
///   // EXTRA_COMPILE_CMD=pkg-config --cflags ACE
///   // EXTRA_LINK_CMD=pkg-config --libs ACE
///   // EXTRA_COMPILE_SHELL=echo -DLEVEL2_CACHE_LINESIZE=$(getconf LEVEL2_CACHE_LINESIZE)
///   // EXTRA_LINK_SHELL=echo -L$(brew --prefix openssl)/lib
///
/// `EXTRA_*_FLAGS_*` values are literal flags (with backtick expansion).
/// `EXTRA_*_CMD` values are executed as a subprocess (no shell) and stdout is used as flags.
/// `EXTRA_*_SHELL` values are executed via `sh -c` and stdout is used as flags.
fn parse_source_flags(source: &Path) -> Result<SourceFlags> {
    let content = fs::read_to_string(source)
        .context(format!("Failed to read source file: {}", source.display()))?;

    let mut flags = SourceFlags::default();

    let args_var_names = [
        "EXTRA_COMPILE_FLAGS_BEFORE",
        "EXTRA_COMPILE_FLAGS_AFTER",
        "EXTRA_LINK_FLAGS_BEFORE",
        "EXTRA_LINK_FLAGS_AFTER",
    ];

    let cmd_var_names = [
        "EXTRA_COMPILE_CMD",
        "EXTRA_LINK_CMD",
    ];

    let shell_var_names = [
        "EXTRA_COMPILE_SHELL",
        "EXTRA_LINK_SHELL",
    ];

    for line in content.lines() {
        let trimmed = line.trim();

        // Try // comment style
        let value_part = if let Some(rest) = trimmed.strip_prefix("//") {
            Some(rest.trim())
        }
        // Try /* ... */ comment style (single-line)
        else if let Some(rest) = trimmed.strip_prefix("/*") {
            rest.strip_suffix("*/").map(|s| s.trim())
        }
        // Try block comment continuation line: * EXTRA_...
        else if let Some(rest) = trimmed.strip_prefix('*') {
            let rest = rest.trim();
            // Skip closing */ lines and empty * lines
            if rest.is_empty() || rest == "/" {
                None
            } else {
                // Strip optional trailing */
                Some(rest.strip_suffix("*/").map(|s| s.trim()).unwrap_or(rest))
            }
        } else {
            None
        };

        let Some(value_part) = value_part else {
            continue;
        };

        for var_name in &args_var_names {
            if let Some(rest) = value_part.strip_prefix(var_name) {
                if let Some(raw_value) = rest.strip_prefix('=') {
                    let expanded = expand_backticks(raw_value.trim())?;
                    let args: Vec<String> = expanded
                        .split_whitespace()
                        .map(String::from)
                        .collect();
                    match *var_name {
                        "EXTRA_COMPILE_FLAGS_BEFORE" => flags.compile_args_before.extend(args),
                        "EXTRA_COMPILE_FLAGS_AFTER" => flags.compile_args_after.extend(args),
                        "EXTRA_LINK_FLAGS_BEFORE" => flags.link_args_before.extend(args),
                        "EXTRA_LINK_FLAGS_AFTER" => flags.link_args_after.extend(args),
                        _ => {}
                    }
                }
            }
        }

        for var_name in &cmd_var_names {
            if let Some(rest) = value_part.strip_prefix(var_name) {
                if let Some(raw_value) = rest.strip_prefix('=') {
                    let cmd = raw_value.trim();
                    let args = cached_shell_command(cmd, run_command_for_flags)?;
                    match *var_name {
                        "EXTRA_COMPILE_CMD" => flags.compile_args_after.extend(args),
                        "EXTRA_LINK_CMD" => flags.link_args_after.extend(args),
                        _ => {}
                    }
                }
            }
        }

        for var_name in &shell_var_names {
            if let Some(rest) = value_part.strip_prefix(var_name) {
                if let Some(raw_value) = rest.strip_prefix('=') {
                    let cmd = raw_value.trim();
                    let args = cached_shell_command(cmd, run_shell_for_flags)?;
                    match *var_name {
                        "EXTRA_COMPILE_SHELL" => flags.compile_args_after.extend(args),
                        "EXTRA_LINK_SHELL" => flags.link_args_after.extend(args),
                        _ => {}
                    }
                }
            }
        }
    }

    Ok(flags)
}

/// Run a command as a subprocess and return its stdout split into args.
/// The value is split on whitespace: first token is the program, rest are arguments.
fn run_command_for_flags(cmd_line: &str) -> Result<Vec<String>> {
    let parts: Vec<&str> = cmd_line.split_whitespace().collect();
    if parts.is_empty() {
        return Ok(Vec::new());
    }
    let program = parts[0];
    let args = &parts[1..];

    let mut cmd = Command::new(program);
    cmd.args(args);
    let output = run_command(&mut cmd)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Command failed: {} — {}", cmd_line, stderr);
    }

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(stdout.split_whitespace().map(String::from).collect())
}

/// Run a command via `sh -c` and return stdout split into flags.
fn run_shell_for_flags(cmd_line: &str) -> Result<Vec<String>> {
    if cmd_line.is_empty() {
        return Ok(Vec::new());
    }

    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(cmd_line);
    let output = run_command(&mut cmd)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Shell command failed: {} — {}", cmd_line, stderr);
    }

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(stdout.split_whitespace().map(String::from).collect())
}

/// Run a backtick command and return its output as a single string.
fn run_backtick_command(cmd_str: &str) -> Result<Vec<String>> {
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(cmd_str);
    let output = run_command(&mut cmd)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Backtick command failed: {} — {}", cmd_str, stderr);
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    // Return as single-element vec so caching works uniformly
    Ok(vec![stdout])
}

/// Expand backtick-wrapped portions in a string by running them as shell commands.
/// E.g. "`pkg-config --cflags gtk+-3.0`" → the stdout of that command.
/// Results are cached to avoid running the same command multiple times.
fn expand_backticks(value: &str) -> Result<String> {
    if !value.contains('`') {
        return Ok(value.to_string());
    }

    let mut result = String::new();
    let mut rest = value;

    while let Some(start) = rest.find('`') {
        result.push_str(&rest[..start]);
        let after_start = &rest[start + 1..];
        let end = after_start.find('`').ok_or_else(|| {
            anyhow::anyhow!("Unmatched backtick in value: {}", value)
        })?;
        let cmd_str = &after_start[..end];
        // Use cache for backtick commands too
        let cached = cached_shell_command(cmd_str, run_backtick_command)?;
        let stdout = cached.first().map(|s| s.as_str()).unwrap_or("");
        result.push_str(stdout);
        rest = &after_start[end + 1..];
    }
    result.push_str(rest);

    Ok(result)
}

pub struct CcProcessor {
    project_root: PathBuf,
    config: CcConfig,
    output_dir: PathBuf,
    deps_dir: PathBuf,
    verbose: bool,
}

impl CcProcessor {
    pub fn new(project_root: PathBuf, config: CcConfig, verbose: bool) -> Self {
        let output_dir = PathBuf::from("out/cc_single_file");
        let deps_dir = PathBuf::from(".rsb/deps");
        Self {
            project_root,
            config,
            output_dir,
            deps_dir,
            verbose,
        }
    }

    /// Get the source directory from scan config (relative path)
    fn source_dir(&self) -> PathBuf {
        scan_root(&self.config.scan)
    }

    /// Check if cc processing should be enabled
    fn should_process(&self) -> bool {
        let src = self.source_dir();
        src.as_os_str().is_empty() || src.exists()
    }

    /// Find all C/C++ source files. Returns (path, is_cpp) pairs.
    fn find_source_files(&self, file_index: &FileIndex) -> Vec<(PathBuf, bool)> {
        file_index.scan(&self.config.scan, true)
            .into_iter()
            .map(|p| {
                let is_cpp = p.extension().and_then(|s| s.to_str()) == Some("cc");
                (p, is_cpp)
            })
            .collect()
    }

    /// Get executable path for a source file.
    /// Mirrors directory structure: src/a/b.c -> out/cc/a/b.elf (suffix is configurable)
    fn get_executable_path(&self, source: &Path) -> PathBuf {
        let source_dir = self.source_dir();
        let relative = if source_dir.as_os_str().is_empty() {
            source.to_path_buf()
        } else {
            source.strip_prefix(&source_dir).unwrap_or(source).to_path_buf()
        };
        let stem = relative.with_extension("");
        let name = format!("{}{}", stem.display(), self.config.output_suffix);
        self.output_dir.join(name)
    }

    /// Get deps file path for a source file.
    /// src/a/b.c -> .rsb/deps/a/b.c.d
    fn get_deps_path(&self, source: &Path) -> PathBuf {
        let source_dir = self.source_dir();
        let relative = if source_dir.as_os_str().is_empty() {
            source.to_path_buf()
        } else {
            source.strip_prefix(&source_dir).unwrap_or(source).to_path_buf()
        };
        let deps_name = format!(
            "{}.d",
            relative.display()
        );
        self.deps_dir.join(deps_name)
    }

    /// Try to read cached dependency info from a .d file.
    /// Returns None if the cache is stale or missing.
    fn read_cached_deps(&self, source: &Path) -> Option<Vec<PathBuf>> {
        let deps_path = self.get_deps_path(source);
        if !deps_path.exists() {
            return None;
        }

        let deps_mtime = fs::metadata(&deps_path).ok()?.modified().ok()?;
        let source_mtime = fs::metadata(source).ok()?.modified().ok()?;

        // If source is newer than deps file, cache is stale
        if source_mtime > deps_mtime {
            return None;
        }

        let content = fs::read_to_string(&deps_path).ok()?;
        let headers = self.parse_dep_file(&content);

        // Check each header still exists and isn't newer than the deps file
        for header in &headers {
            let meta = fs::metadata(header).ok()?;
            let header_mtime = meta.modified().ok()?;
            if header_mtime > deps_mtime {
                return None;
            }
        }

        Some(headers)
    }

    /// Add include paths and compile flags (before, base, after) to a command.
    /// Shared between `scan_dependencies()` and `compile_source()`.
    fn add_compile_flags(&self, cmd: &mut Command, is_cpp: bool, source_flags: &SourceFlags) {
        let flags = if is_cpp { &self.config.cxxflags } else { &self.config.cflags };
        for inc in &self.config.include_paths {
            cmd.arg(format!("-I{}", inc));
        }
        for arg in &source_flags.compile_args_before {
            cmd.arg(arg);
        }
        for flag in flags {
            cmd.arg(flag);
        }
        for arg in &source_flags.compile_args_after {
            cmd.arg(arg);
        }
    }

    /// Run gcc/g++ -MM to scan dependencies for a source file.
    /// Also writes the result to the deps cache.
    fn scan_dependencies(&self, source: &Path, is_cpp: bool) -> Result<Vec<PathBuf>> {
        let compiler = if is_cpp { &self.config.cxx } else { &self.config.cc };
        let source_flags = parse_source_flags(source)?;

        let mut cmd = Command::new(compiler);
        cmd.arg("-MM");
        self.add_compile_flags(&mut cmd, is_cpp, &source_flags);
        cmd.arg(source);
        cmd.current_dir(&self.project_root);

        if self.verbose {
            println!("[cc_single_file] {}", format_command(&cmd));
        }

        let output = run_command(&mut cmd)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Dependency scan failed for {}: {}", source.display(), stderr);
        }

        let content = String::from_utf8_lossy(&output.stdout).to_string();
        let headers = self.parse_dep_file(&content);

        // Cache the deps file
        let deps_path = self.get_deps_path(source);
        if let Some(parent) = deps_path.parent() {
            fs::create_dir_all(parent)
                .context("Failed to create deps cache directory")?;
        }
        fs::write(&deps_path, &content)
            .context(format!("Failed to write deps file: {}", deps_path.display()))?;

        Ok(headers)
    }

    /// Parse a Makefile-style dependency file (.d) produced by gcc -MM.
    /// Format: target.o: source.c header1.h header2.h \
    ///           header3.h
    /// Returns the list of header files (excludes the source file itself and system headers).
    /// All returned paths are relative to project root.
    fn parse_dep_file(&self, content: &str) -> Vec<PathBuf> {
        // Join continuation lines (backslash-newline)
        let joined = content.replace("\\\n", " ");

        // Find the colon separating target from dependencies
        let deps_part = match joined.find(':') {
            Some(pos) => &joined[pos + 1..],
            None => return Vec::new(),
        };

        // Split by whitespace, skip the first token (the source file itself)
        let tokens: Vec<&str> = deps_part.split_whitespace().collect();
        if tokens.is_empty() {
            return Vec::new();
        }

        // First token is the source file; remaining are headers
        tokens[1..]
            .iter()
            .filter_map(|token| {
                let path = Path::new(token);
                // Filter out system headers (absolute paths starting with /usr/ or /lib/)
                let path_str = path.to_string_lossy();
                if path_str.starts_with("/usr/") || path_str.starts_with("/lib/") {
                    return None;
                }
                // Skip other absolute paths (system headers)
                if path.is_absolute() {
                    return None;
                }
                // Keep relative paths as-is
                Some(path.to_path_buf())
            })
            .collect()
    }

    /// Compile a single source file directly to an executable.
    fn compile_source(&self, source: &Path, executable: &Path, deps_file: &Path, is_cpp: bool) -> Result<()> {
        let compiler = if is_cpp { &self.config.cxx } else { &self.config.cc };
        let source_flags = parse_source_flags(source)?;

        // Ensure output directory exists
        if let Some(parent) = executable.parent() {
            fs::create_dir_all(parent)
                .context("Failed to create output directory")?;
        }

        // Ensure deps directory exists
        if let Some(parent) = deps_file.parent() {
            fs::create_dir_all(parent)
                .context("Failed to create deps directory")?;
        }

        let mut cmd = Command::new(compiler);
        cmd.arg("-MMD").arg("-MF").arg(deps_file);
        self.add_compile_flags(&mut cmd, is_cpp, &source_flags);
        cmd.arg("-o").arg(executable).arg(source);
        for arg in &source_flags.link_args_before {
            cmd.arg(arg);
        }
        for flag in &self.config.ldflags {
            cmd.arg(flag);
        }
        for arg in &source_flags.link_args_after {
            cmd.arg(arg);
        }
        cmd.current_dir(&self.project_root);

        if self.verbose {
            println!("[cc_single_file] {}", format_command(&cmd));
        }

        let output = run_command(&mut cmd)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Compilation failed for {}: {}", source.display(), stderr);
        }

        Ok(())
    }
}

impl ProductDiscovery for CcProcessor {
    fn description(&self) -> &str {
        "Compile C/C++ source files into executables (single-file)"
    }

    fn processor_type(&self) -> crate::processors::ProcessorType {
        crate::processors::ProcessorType::Generator
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        self.should_process() && !self.find_source_files(file_index).is_empty()
    }

    fn required_tools(&self) -> Vec<String> {
        vec![self.config.cc.clone(), self.config.cxx.clone()]
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex) -> Result<()> {
        if !self.should_process() {
            return Ok(());
        }

        let source_files = self.find_source_files(file_index);
        if source_files.is_empty() {
            return Ok(());
        }

        let config_hash = Some(config_hash(&self.config));
        let extra = resolve_extra_inputs(&self.config.extra_inputs)?;

        // Show progress bar for dependency scanning
        let pb = indicatif::ProgressBar::new(source_files.len() as u64);
        pb.set_style(
            indicatif::ProgressStyle::default_bar()
                .template("[cc_single_file] Scanning dependencies {bar:40} {pos}/{len} {msg}")
                .unwrap()
                .progress_chars("##-")
        );

        for (source, is_cpp) in &source_files {
            pb.set_message(source.display().to_string());

            let executable = self.get_executable_path(source);

            // Resolve header dependencies
            let headers = match self.read_cached_deps(source) {
                Some(h) => h,
                None => self.scan_dependencies(source, *is_cpp)
                    .unwrap_or_default(),
            };

            // Build inputs: source file + all headers + extra inputs
            let mut inputs = vec![source.clone()];
            inputs.extend(headers);
            inputs.extend(extra.clone());

            graph.add_product(
                inputs,
                vec![executable],
                "cc_single_file",
                config_hash.clone(),
            )?;
            pb.inc(1);
        }
        pb.finish_and_clear();

        Ok(())
    }

    fn execute(&self, product: &Product) -> Result<()> {
        let source = &product.inputs[0];
        let executable = &product.outputs[0];
        let is_cpp = source.extension().and_then(|s| s.to_str()) == Some("cc");
        let deps_file = self.get_deps_path(source);
        self.compile_source(source, executable, &deps_file, is_cpp)
    }

    fn clean(&self, product: &Product) -> Result<()> {
        clean_outputs(product, "cc_single_file")
    }
}
