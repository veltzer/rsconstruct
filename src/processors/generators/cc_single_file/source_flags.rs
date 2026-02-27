use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::Command;
use parking_lot::Mutex;

use crate::processors::{check_command_output, run_command_capture};

/// Global cache for shell command results to avoid running the same command multiple times.
/// Key is the command string, value is the resulting flags.
/// Thread-safe via `parking_lot::Mutex`. Lives for the process lifetime (never cleared).
static SHELL_COMMAND_CACHE: Mutex<Option<HashMap<String, Vec<String>>>> = Mutex::new(None);

/// Get or compute flags from a shell command, caching the result.
fn cached_shell_command(cmd_line: &str, runner: impl FnOnce(&str) -> Result<Vec<String>>) -> Result<Vec<String>> {
    let mut guard = SHELL_COMMAND_CACHE.lock();
    let cache = guard.get_or_insert_with(HashMap::new);

    if let Some(cached) = cache.get(cmd_line) {
        return Ok(cached.clone());
    }

    let result = runner(cmd_line)?;
    cache.insert(cmd_line.to_string(), result.clone());
    Ok(result)
}

/// Per-file compile/link flags extracted from source comments.
#[derive(Default)]
pub(super) struct SourceFlags {
    pub(super) compile_args_before: Vec<String>,
    pub(super) compile_args_after: Vec<String>,
    pub(super) link_args_before: Vec<String>,
    pub(super) link_args_after: Vec<String>,
}

/// Extract the comment value from a C/C++ source line.
///
/// Handles three comment styles:
///   - `// ...` — returns the text after `//`
///   - `/* ... */` — returns the text between `/*` and `*/`
///   - `* ...` — block comment continuation line, returns the text after `*`
///
/// Returns None for non-comment lines or empty/closing block comment lines.
fn extract_comment_value(line: &str) -> Option<&str> {
    let trimmed = line.trim();

    // Try // comment style
    if let Some(rest) = trimmed.strip_prefix("//") {
        return Some(rest.trim());
    }
    // Try /* ... */ comment style (single-line)
    if let Some(rest) = trimmed.strip_prefix("/*") {
        return rest.strip_suffix("*/").map(|s| s.trim());
    }
    // Try block comment continuation line: * ...
    if let Some(rest) = trimmed.strip_prefix('*') {
        let rest = rest.trim();
        if rest.is_empty() || rest == "/" {
            return None;
        }
        return Some(rest.strip_suffix("*/").map(|s| s.trim()).unwrap_or(rest));
    }
    None
}

/// Check if a source file should be excluded from a specific compiler profile.
///
/// Looks for directives like:
///   // EXCLUDE_PROFILE=clang
///   // EXCLUDE_PROFILE=gcc clang
///
/// Returns true if the file should be excluded for the given profile.
pub(super) fn should_exclude_for_profile(source: &Path, profile_name: &str) -> bool {
    // If profile name is empty (single compiler setup), never exclude
    if profile_name.is_empty() {
        return false;
    }

    let content = match fs::read_to_string(source) {
        Ok(c) => c,
        Err(_) => return false,
    };

    for line in content.lines() {
        let Some(value_part) = extract_comment_value(line) else {
            continue;
        };

        if let Some(rest) = value_part.strip_prefix("EXCLUDE_PROFILE")
            && let Some(profiles_str) = rest.strip_prefix('=') {
                let excluded_profiles: Vec<&str> = profiles_str.split_whitespace().collect();
                if excluded_profiles.contains(&profile_name) {
                    return true;
                }
            }
    }

    false
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
/// Compiler profile-specific flags (only applied when compiling with the named profile):
///   // EXTRA_COMPILE_FLAGS_BEFORE[gcc]=-femit-struct-debug-baseonly
///   // EXTRA_COMPILE_FLAGS_BEFORE[clang]=-gline-tables-only
///
/// Exclude file from specific profiles:
///   // EXCLUDE_PROFILE=clang
///   // EXCLUDE_PROFILE=gcc clang
///
/// `EXTRA_*_FLAGS_*` values are literal flags (with backtick expansion).
/// `EXTRA_*_CMD` values are executed as a subprocess (no shell) and stdout is used as flags.
/// `EXTRA_*_SHELL` values are executed via `sh -c` and stdout is used as flags.
///
/// The `profile_name` parameter specifies the current compiler profile name.
/// Directives without a profile suffix apply to all profiles.
/// Directives with a profile suffix (e.g., `[gcc]`) only apply when that profile is active.
pub(super) fn parse_source_flags(source: &Path, profile_name: &str) -> Result<SourceFlags> {
    let content = fs::read_to_string(source)
        .with_context(|| format!("Failed to read source file: {}", source.display()))?;

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
        let Some(value_part) = extract_comment_value(line) else {
            continue;
        };

        for var_name in &args_var_names {
            if let Some((rest, applies)) = match_directive_with_profile(value_part, var_name, profile_name)
                && applies
                    && let Some(raw_value) = rest.strip_prefix('=') {
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

        for var_name in &cmd_var_names {
            if let Some((rest, applies)) = match_directive_with_profile(value_part, var_name, profile_name)
                && applies
                    && let Some(raw_value) = rest.strip_prefix('=') {
                        let cmd = raw_value.trim();
                        let args = cached_shell_command(cmd, run_command_for_flags)?;
                        match *var_name {
                            "EXTRA_COMPILE_CMD" => flags.compile_args_after.extend(args),
                            "EXTRA_LINK_CMD" => flags.link_args_after.extend(args),
                            _ => {}
                        }
                    }
        }

        for var_name in &shell_var_names {
            if let Some((rest, applies)) = match_directive_with_profile(value_part, var_name, profile_name)
                && applies
                    && let Some(raw_value) = rest.strip_prefix('=') {
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

    Ok(flags)
}

/// Match a directive with optional profile suffix.
/// Returns Some((rest_of_line, applies)) if the directive matches.
/// - `applies` is true if the directive applies to the current profile
///   (either no profile suffix, or profile suffix matches current profile)
/// - `rest_of_line` is everything after the directive name (and optional profile suffix)
///
/// Examples:
///   "EXTRA_COMPILE_FLAGS_BEFORE=-g" with profile "gcc" -> Some(("=-g", true))
///   "EXTRA_COMPILE_FLAGS_BEFORE[gcc]=-g" with profile "gcc" -> Some(("=-g", true))
///   "EXTRA_COMPILE_FLAGS_BEFORE[clang]=-g" with profile "gcc" -> Some(("=-g", false))
fn match_directive_with_profile<'a>(line: &'a str, directive: &str, profile_name: &str) -> Option<(&'a str, bool)> {
    if let Some(rest) = line.strip_prefix(directive) {
        // Check for profile suffix [profile_name]
        if let Some(rest_after_bracket) = rest.strip_prefix('[') {
            // Find closing bracket
            if let Some(bracket_end) = rest_after_bracket.find(']') {
                let specified_profile = &rest_after_bracket[..bracket_end];
                let rest_after_suffix = &rest_after_bracket[bracket_end + 1..];
                let applies = specified_profile == profile_name;
                return Some((rest_after_suffix, applies));
            }
            // Malformed (no closing bracket), don't match
            None
        } else {
            // No profile suffix - applies to all profiles
            Some((rest, true))
        }
    } else {
        None
    }
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
    let output = run_command_capture(&mut cmd)?;
    check_command_output(&output, cmd_line)?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    Ok(stdout.split_whitespace().map(String::from).collect())
}

/// Run a command via `sh -c` and return stdout split into flags.
fn run_shell_for_flags(cmd_line: &str) -> Result<Vec<String>> {
    if cmd_line.is_empty() {
        return Ok(Vec::new());
    }

    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(cmd_line);
    let output = run_command_capture(&mut cmd)?;
    check_command_output(&output, cmd_line)?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    Ok(stdout.split_whitespace().map(String::from).collect())
}

/// Run a backtick command and return its output as a single string.
fn run_backtick_command(cmd_str: &str) -> Result<Vec<String>> {
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(cmd_str);
    let output = run_command_capture(&mut cmd)?;
    check_command_output(&output, cmd_str)?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
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
