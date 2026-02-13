//! C/C++ dependency analyzer for scanning header files.
//!
//! Scans source files for #include directives and adds header dependencies
//! to products in the build graph.

use anyhow::Result;
use regex::Regex;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use crate::config::{CppAnalyzerConfig, IncludeScanner};
use crate::errors;
use crate::deps_cache::DepsCache;
use crate::file_index::FileIndex;
use crate::graph::BuildGraph;
use crate::processors::{format_command, run_command_capture};

use super::DepAnalyzer;

/// C/C++ dependency analyzer that scans source files for #include directives.
pub struct CppDepAnalyzer {
    config: CppAnalyzerConfig,
    verbose: bool,
    /// Cached canonical project root path (for stripping absolute prefixes from compiler output)
    canonical_root: OnceLock<PathBuf>,
    /// Cached system include paths from the C compiler
    system_include_paths_c: OnceLock<Vec<PathBuf>>,
    /// Cached system include paths from the C++ compiler
    system_include_paths_cxx: OnceLock<Vec<PathBuf>>,
    /// Cached include paths from pkg-config
    pkg_config_include_paths: OnceLock<Vec<PathBuf>>,
    /// Cached include paths from include_path_commands
    command_include_paths: OnceLock<Vec<PathBuf>>,
}

impl CppDepAnalyzer {
    pub fn new(config: CppAnalyzerConfig, verbose: bool) -> Self {
        Self {
            config,
            verbose,
            canonical_root: OnceLock::new(),
            system_include_paths_c: OnceLock::new(),
            system_include_paths_cxx: OnceLock::new(),
            pkg_config_include_paths: OnceLock::new(),
            command_include_paths: OnceLock::new(),
        }
    }

    /// Get the canonical project root path (lazily computed).
    fn canonical_root(&self) -> &Path {
        self.canonical_root.get_or_init(|| {
            Path::new(".").canonicalize().unwrap_or_else(|_| PathBuf::from("."))
        })
    }

    /// Query pkg-config for include paths from configured packages.
    /// Uses `pkg-config --cflags-only-I` and strips the -I prefix.
    fn get_pkg_config_include_paths(&self) -> &[PathBuf] {
        self.pkg_config_include_paths.get_or_init(|| {
            if self.config.pkg_config.is_empty() {
                return Vec::new();
            }

            let mut cmd = Command::new("pkg-config");
            cmd.arg("--cflags-only-I");
            cmd.args(&self.config.pkg_config);

            if self.verbose {
                eprintln!("[cpp] Querying pkg-config: {}", format_command(&cmd));
            }

            let output = match run_command_capture(&mut cmd) {
                Ok(o) => o,
                Err(e) => {
                    eprintln!("[cpp] Failed to query pkg-config: {}", e);
                    return Vec::new();
                }
            };

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                eprintln!("[cpp] pkg-config failed: {}", stderr.trim());
                return Vec::new();
            }

            let stdout = String::from_utf8_lossy(&output.stdout);
            let paths: Vec<PathBuf> = stdout
                .split_whitespace()
                .filter_map(|flag| {
                    // Strip -I prefix
                    flag.strip_prefix("-I").map(PathBuf::from)
                })
                .collect();

            if self.verbose && !paths.is_empty() {
                eprintln!("[cpp] Found {} include paths from pkg-config", paths.len());
            }

            paths
        })
    }

    /// Run configured include_path_commands and collect their output as include paths.
    /// Each command is executed via `sh -c` and its stdout (trimmed) is added as an include path.
    /// This supports shell syntax like command substitution: "echo $(gcc -print-file-name=plugin)/include"
    fn get_command_include_paths(&self) -> &[PathBuf] {
        self.command_include_paths.get_or_init(|| {
            if self.config.include_path_commands.is_empty() {
                return Vec::new();
            }

            let mut paths = Vec::new();

            for cmd_str in &self.config.include_path_commands {
                if cmd_str.trim().is_empty() {
                    continue;
                }

                // Run via shell to support shell syntax (command substitution, etc.)
                let mut cmd = Command::new("sh");
                cmd.arg("-c");
                cmd.arg(cmd_str);

                if self.verbose {
                    eprintln!("[cpp] Running include path command: sh -c '{}'", cmd_str);
                }

                let output = match run_command_capture(&mut cmd) {
                    Ok(o) => o,
                    Err(e) => {
                        eprintln!("[cpp] Failed to run '{}': {}", cmd_str, e);
                        continue;
                    }
                };

                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    eprintln!("[cpp] Command '{}' failed: {}", cmd_str, stderr.trim());
                    continue;
                }

                let stdout = String::from_utf8_lossy(&output.stdout);
                let path_str = stdout.trim();

                if path_str.is_empty() {
                    continue;
                }

                let path = PathBuf::from(path_str);

                // Check if the path exists and is a directory
                if path.is_dir() {
                    if self.verbose {
                        eprintln!("[cpp] Added include path from command: {}", path.display());
                    }
                    paths.push(path);
                } else if self.verbose {
                    eprintln!("[cpp] Command output is not a directory: {}", path_str);
                }
            }

            if self.verbose && !paths.is_empty() {
                eprintln!("[cpp] Found {} include paths from commands", paths.len());
            }

            paths
        })
    }

    /// Query the compiler for its default include search paths.
    /// Uses `compiler -E -Wp,-v -xc /dev/null` (or -xc++ for C++).
    /// Returns the list of system include directories.
    fn get_system_include_paths(&self, is_cpp: bool) -> &[PathBuf] {
        let cache = if is_cpp {
            &self.system_include_paths_cxx
        } else {
            &self.system_include_paths_c
        };

        cache.get_or_init(|| {
            let compiler = if is_cpp { &self.config.cxx } else { &self.config.cc };
            let lang_flag = if is_cpp { "-xc++" } else { "-xc" };

            let mut cmd = Command::new(compiler);
            cmd.args(["-E", "-Wp,-v", lang_flag, "/dev/null"]);

            if self.verbose {
                eprintln!("[cpp] Querying {} include paths: {}", compiler, format_command(&cmd));
            }

            let output = match run_command_capture(&mut cmd) {
                Ok(o) => o,
                Err(e) => {
                    if self.verbose {
                        eprintln!("[cpp] Failed to query compiler include paths: {}", e);
                    }
                    return Vec::new();
                }
            };

            // The include paths are printed to stderr between lines:
            // "#include <...> search starts here:"
            // and
            // "End of search list."
            let stderr = String::from_utf8_lossy(&output.stderr);
            let mut paths = Vec::new();
            let mut in_search_list = false;

            for line in stderr.lines() {
                let trimmed = line.trim();
                if trimmed.contains("#include <...> search starts here:") ||
                   trimmed.contains("#include \"...\" search starts here:") {
                    in_search_list = true;
                    continue;
                }
                if trimmed == "End of search list." {
                    break;
                }
                if in_search_list && !trimmed.is_empty() {
                    // Remove any trailing annotations like "(framework directory)"
                    let path_str = trimmed.split_whitespace().next().unwrap_or(trimmed);
                    let path = PathBuf::from(path_str);
                    // Canonicalize to resolve symlinks
                    if let Ok(canonical) = path.canonicalize() {
                        paths.push(canonical);
                    } else {
                        paths.push(path);
                    }
                }
            }

            if self.verbose && !paths.is_empty() {
                eprintln!("[cpp] Found {} system include paths", paths.len());
            }

            paths
        })
    }

    /// Check if a path is within the project root (not a system header).
    fn is_project_local(&self, path: &Path) -> bool {
        // Canonicalize to resolve symlinks and get absolute path
        let canonical = match path.canonicalize() {
            Ok(p) => p,
            Err(_) => return false, // Can't canonicalize, probably doesn't exist
        };

        // Check if it starts with the project root
        canonical.starts_with(self.canonical_root())
    }

    /// Check if a path should be excluded based on exclude_dirs config.
    fn is_excluded(&self, path: &Path) -> bool {
        if self.config.exclude_dirs.is_empty() {
            return false;
        }
        let path_str = path.to_string_lossy();
        self.config.exclude_dirs.iter().any(|dir| path_str.contains(dir))
    }

    /// Native regex-based include scanner.
    /// Scans source files for #include directives and recursively follows them.
    /// Returns all header files that the source depends on.
    fn scan_dependencies_native(&self, source: &Path, is_cpp: bool) -> Result<Vec<PathBuf>> {
        let mut visited: HashSet<PathBuf> = HashSet::new();
        let mut headers: Vec<PathBuf> = Vec::new();
        let mut headers_set: HashSet<PathBuf> = HashSet::new();

        // Build include search paths:
        // 1. Source file's directory (for "" includes)
        // 2. Configured include_paths (-I flags)
        // 3. pkg-config include paths
        // 4. Project root
        // 5. System include paths from compiler (for <> includes)
        let source_dir = source.parent().unwrap_or(Path::new("."));
        let mut search_paths = vec![source_dir.to_path_buf()];
        for inc in &self.config.include_paths {
            search_paths.push(PathBuf::from(inc));
        }
        // Add pkg-config include paths
        for inc in self.get_pkg_config_include_paths() {
            search_paths.push(inc.clone());
        }
        // Add include paths from commands
        for inc in self.get_command_include_paths() {
            search_paths.push(inc.clone());
        }
        // Also search from project root for project-relative includes
        search_paths.push(PathBuf::new());

        // Get system include paths from the compiler
        let system_paths = self.get_system_include_paths(is_cpp);

        self.scan_includes_recursive(source, &search_paths, system_paths, &mut visited, &mut headers, &mut headers_set)?;

        Ok(headers)
    }

    /// Recursively scan a file for #include directives
    fn scan_includes_recursive(
        &self,
        file: &Path,
        search_paths: &[PathBuf],
        system_paths: &[PathBuf],
        visited: &mut HashSet<PathBuf>,
        headers: &mut Vec<PathBuf>,
        headers_set: &mut HashSet<PathBuf>,
    ) -> Result<()> {
        // Normalize path to avoid visiting same file twice
        let canonical = match file.canonicalize() {
            Ok(p) => p,
            Err(_) => file.to_path_buf(),
        };

        if visited.contains(&canonical) {
            return Ok(());
        }
        visited.insert(canonical.clone());

        let content = fs::read_to_string(file)
            .map_err(|e| anyhow::anyhow!("Failed to read {}: {}", file.display(), e))?;

        // Regex to match #include "file" and #include <file>
        // Captures: 1 = bracket type (< or "), 2 = path
        static INCLUDE_RE: OnceLock<Regex> = OnceLock::new();
        let include_re = INCLUDE_RE.get_or_init(|| {
            Regex::new(r#"^\s*#\s*include\s*([<"])([^>"]+)[>"]"#).expect(errors::INVALID_REGEX)
        });

        for line in content.lines() {
            if let Some(caps) = include_re.captures(line) {
                let bracket = &caps[1];
                let include_path = &caps[2];

                // Skip absolute paths in the include directive itself
                if include_path.starts_with('/') {
                    continue;
                }

                // For angle-bracket includes without file extension and no path separator,
                // assume C++ standard library header (e.g., <vector>, <string>, <iostream>)
                let is_angle_bracket = bracket == "<";
                if is_angle_bracket && !include_path.contains('.') && !include_path.contains('/') {
                    continue;
                }

                // Try to find the included file in search paths
                // For "" includes, search relative to current file's directory first
                // For <> includes, search in include_paths and system paths
                let found = self.find_include(include_path, file.parent(), search_paths, system_paths, is_angle_bracket);

                if let Some(header_path) = found {
                    // Only track headers that are within the project root
                    if self.is_project_local(&header_path) {
                        let relative = if header_path.is_absolute() {
                            // Try to strip project root prefix
                            let canonical_root = self.canonical_root();
                            if let Ok(rel) = header_path.strip_prefix(canonical_root) {
                                rel.to_path_buf()
                            } else if let Ok(canonical) = header_path.canonicalize() {
                                canonical.strip_prefix(canonical_root)
                                    .map(|p| p.to_path_buf())
                                    .unwrap_or_else(|_| header_path.clone())
                            } else {
                                header_path.clone()
                            }
                        } else {
                            header_path.clone()
                        };

                        if headers_set.insert(relative.clone()) {
                            headers.push(relative);
                        }

                        // Recursively scan this header
                        self.scan_includes_recursive(&header_path, search_paths, system_paths, visited, headers, headers_set)?;
                    }
                    // If found but not project-local (system header), that's fine - skip it
                } else {
                    // Include not found anywhere - this is an error
                    let bracket_close = if is_angle_bracket { ">" } else { "\"" };
                    let bracket_open = if is_angle_bracket { "<" } else { "\"" };
                    anyhow::bail!(
                        "Include not found: #include {}{}{} in {}",
                        bracket_open, include_path, bracket_close,
                        file.display()
                    );
                }
            }
        }

        Ok(())
    }

    /// Find an include file in the search paths
    /// For "" includes (is_angle_bracket=false), searches current directory first
    /// For <> includes (is_angle_bracket=true), searches include paths then system paths
    fn find_include(
        &self,
        include: &str,
        current_dir: Option<&Path>,
        search_paths: &[PathBuf],
        system_paths: &[PathBuf],
        is_angle_bracket: bool,
    ) -> Option<PathBuf> {
        // For #include "file", first try relative to current file's directory
        // For #include <file>, skip this step
        if !is_angle_bracket
            && let Some(dir) = current_dir {
                let candidate = dir.join(include);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }

        // Then try each configured search path (for both "" and <> includes)
        for search in search_paths {
            let candidate = if search.as_os_str().is_empty() {
                PathBuf::from(include)
            } else {
                search.join(include)
            };
            if candidate.is_file() {
                return Some(candidate);
            }
        }

        // For <> includes, also search system include paths from compiler
        if is_angle_bracket {
            for search in system_paths {
                let candidate = search.join(include);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }

        None
    }

    /// Run gcc/g++ -MM to scan dependencies for a source file.
    fn scan_dependencies_compiler(&self, source: &Path, is_cpp: bool) -> Result<Vec<PathBuf>> {
        let compiler = if is_cpp { &self.config.cxx } else { &self.config.cc };

        let mut cmd = Command::new(compiler);
        cmd.arg("-MM");

        // Add include paths
        for inc in &self.config.include_paths {
            cmd.arg(format!("-I{}", inc));
        }

        // Add pkg-config include paths
        for inc in self.get_pkg_config_include_paths() {
            cmd.arg(format!("-I{}", inc.display()));
        }

        // Add include paths from commands
        for inc in self.get_command_include_paths() {
            cmd.arg(format!("-I{}", inc.display()));
        }

        // Add compile flags
        let flags = if is_cpp { &self.config.cxxflags } else { &self.config.cflags };
        for flag in flags {
            cmd.arg(flag);
        }

        cmd.arg(source);

        if self.verbose {
            eprintln!("[cpp] {}", format_command(&cmd));
        }

        let output = run_command_capture(&mut cmd)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Dependency scan failed for {}: {}", source.display(), stderr);
        }

        let content = String::from_utf8_lossy(&output.stdout).to_string();
        Ok(self.parse_dep_file(&content))
    }

    /// Parse a Makefile-style dependency file (.d) produced by gcc -MM.
    /// Format: target.o: source.c header1.h header2.h \
    ///           header3.h
    /// Returns the list of header files (excludes the source file itself and system headers).
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
        let canonical_root = self.canonical_root();
        tokens[1..]
            .iter()
            .filter_map(|token| {
                let path = PathBuf::from(token);

                // For absolute paths, check if they're within the project
                if path.is_absolute() {
                    if self.is_project_local(&path) {
                        // Convert to relative path
                        if let Ok(rel) = path.strip_prefix(canonical_root) {
                            Some(rel.to_path_buf())
                        } else if let Ok(canonical) = path.canonicalize() {
                            canonical.strip_prefix(canonical_root)
                                .ok()
                                .map(|p| p.to_path_buf())
                        } else {
                            None
                        }
                    } else {
                        // System header, skip it
                        None
                    }
                } else {
                    // Relative paths are assumed to be project-local
                    Some(path)
                }
            })
            .collect()
    }

    /// Scan dependencies using the configured method (native or compiler).
    fn scan_dependencies(&self, source: &Path, is_cpp: bool) -> Result<Vec<PathBuf>> {
        match self.config.include_scanner {
            IncludeScanner::Native => self.scan_dependencies_native(source, is_cpp),
            IncludeScanner::Compiler => self.scan_dependencies_compiler(source, is_cpp),
        }
    }
}

impl DepAnalyzer for CppDepAnalyzer {
    fn description(&self) -> &str {
        "Scan C/C++ source files for #include dependencies"
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        // Check if there are any C/C++ source files
        let extensions = [".c", ".cc", ".cpp", ".cxx", ".h", ".hh", ".hpp", ".hxx"];
        for ext in extensions {
            if file_index.has_extension(ext) {
                return true;
            }
        }
        false
    }

    fn analyze(&self, graph: &mut BuildGraph, deps_cache: &mut DepsCache, _file_index: &FileIndex, verbose: bool) -> Result<()> {
        let cpp_extensions: HashSet<&str> = [".c", ".cc", ".cpp", ".cxx"].iter().copied().collect();

        super::analyze_with_scanner(
            graph,
            deps_cache,
            "cpp",
            |p| {
                if p.inputs.is_empty() {
                    return None;
                }
                let source = &p.inputs[0];
                if self.is_excluded(source) {
                    return None;
                }
                let ext = source.extension().and_then(|s| s.to_str()).unwrap_or("");
                let ext_with_dot = format!(".{}", ext);
                if cpp_extensions.contains(ext_with_dot.as_str()) {
                    Some(source.clone())
                } else {
                    None
                }
            },
            |source| {
                let ext = source.extension().and_then(|s| s.to_str()).unwrap_or("");
                let is_cpp = ext == "cc" || ext == "cpp" || ext == "cxx";
                self.scan_dependencies(source, is_cpp)
            },
            verbose,
        )
    }
}
