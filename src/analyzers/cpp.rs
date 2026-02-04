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

use crate::config::{CppAnalyzerConfig, IncludeScanner};
use crate::deps_cache::DepsCache;
use crate::file_index::FileIndex;
use crate::graph::BuildGraph;
use crate::processors::{format_command, run_command};

use super::DepAnalyzer;

/// C/C++ dependency analyzer that scans source files for #include directives.
pub struct CppDepAnalyzer {
    config: CppAnalyzerConfig,
    project_root: PathBuf,
    verbose: bool,
}

impl CppDepAnalyzer {
    pub fn new(config: CppAnalyzerConfig, project_root: PathBuf, verbose: bool) -> Self {
        Self {
            config,
            project_root,
            verbose,
        }
    }

    /// Native regex-based include scanner.
    /// Scans source files for #include directives and recursively follows them.
    /// Returns all header files that the source depends on.
    fn scan_dependencies_native(&self, source: &Path) -> Result<Vec<PathBuf>> {
        let mut visited: HashSet<PathBuf> = HashSet::new();
        let mut headers: Vec<PathBuf> = Vec::new();

        // Build include search paths: source directory + configured include_paths
        let source_dir = source.parent().unwrap_or(Path::new("."));
        let mut search_paths = vec![source_dir.to_path_buf()];
        for inc in &self.config.include_paths {
            search_paths.push(PathBuf::from(inc));
        }
        // Also search from project root for project-relative includes
        search_paths.push(PathBuf::new());

        self.scan_includes_recursive(source, &search_paths, &mut visited, &mut headers)?;

        Ok(headers)
    }

    /// Recursively scan a file for #include directives
    fn scan_includes_recursive(
        &self,
        file: &Path,
        search_paths: &[PathBuf],
        visited: &mut HashSet<PathBuf>,
        headers: &mut Vec<PathBuf>,
    ) -> Result<()> {
        // Normalize path to avoid visiting same file twice
        let canonical = file.to_path_buf();

        if visited.contains(&canonical) {
            return Ok(());
        }
        visited.insert(canonical.clone());

        let content = match fs::read_to_string(file) {
            Ok(c) => c,
            Err(_) => return Ok(()), // File doesn't exist or can't be read
        };

        // Regex to match #include "file" and #include <file>
        // Captures: 1 = bracket type (< or "), 2 = path
        let include_re = Regex::new(r#"^\s*#\s*include\s*([<"])([^>"]+)[>"]"#).unwrap();

        for line in content.lines() {
            if let Some(caps) = include_re.captures(line) {
                let bracket = &caps[1];
                let include_path = &caps[2];

                // Skip system headers (absolute paths)
                if include_path.starts_with('/') {
                    continue;
                }

                // For angle-bracket includes without file extension, assume system header
                // (e.g., <vector>, <string>, <iostream>, <cstdio>)
                if bracket == "<" && !include_path.contains('.') && !include_path.contains('/') {
                    continue;
                }

                // Try to find the included file in search paths
                let found = self.find_include(include_path, file.parent(), search_paths);

                if let Some(header_path) = found {
                    // Skip system headers
                    let path_str = header_path.to_string_lossy();
                    if path_str.starts_with("/usr/") || path_str.starts_with("/lib/") {
                        continue;
                    }

                    // Only include if it's a relative path (project-local)
                    if !header_path.is_absolute() || header_path.starts_with(&self.project_root) {
                        let relative = if header_path.is_absolute() {
                            header_path.strip_prefix(&self.project_root)
                                .unwrap_or(&header_path)
                                .to_path_buf()
                        } else {
                            header_path.clone()
                        };

                        if !headers.contains(&relative) {
                            headers.push(relative.clone());
                        }

                        // Recursively scan this header
                        self.scan_includes_recursive(&header_path, search_paths, visited, headers)?;
                    }
                }
            }
        }

        Ok(())
    }

    /// Find an include file in the search paths
    fn find_include(&self, include: &str, current_dir: Option<&Path>, search_paths: &[PathBuf]) -> Option<PathBuf> {
        // First, try relative to current file's directory (for #include "file")
        if let Some(dir) = current_dir {
            let candidate = dir.join(include);
            if candidate.is_file() {
                return Some(candidate);
            }
        }

        // Then try each search path
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

        // Add compile flags
        let flags = if is_cpp { &self.config.cxxflags } else { &self.config.cflags };
        for flag in flags {
            cmd.arg(flag);
        }

        cmd.arg(source);
        cmd.current_dir(&self.project_root);

        if self.verbose {
            eprintln!("[cpp] {}", format_command(&cmd));
        }

        let output = run_command(&mut cmd)?;

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

    /// Scan dependencies using the configured method (native or compiler).
    fn scan_dependencies(&self, source: &Path, is_cpp: bool) -> Result<Vec<PathBuf>> {
        match self.config.include_scanner {
            IncludeScanner::Native => self.scan_dependencies_native(source),
            IncludeScanner::Compiler => self.scan_dependencies_compiler(source, is_cpp),
        }
    }
}

impl DepAnalyzer for CppDepAnalyzer {
    fn name(&self) -> &str {
        "cpp"
    }

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

    fn analyze(&self, graph: &mut BuildGraph, deps_cache: &mut DepsCache, _file_index: &FileIndex) -> Result<()> {
        // Find all products that have C/C++ source files as their primary input
        let cpp_extensions: HashSet<&str> = [".c", ".cc", ".cpp", ".cxx"].iter().copied().collect();

        // Collect products with C/C++ sources
        let products: Vec<(usize, PathBuf, bool)> = graph.products()
            .iter()
            .filter_map(|p| {
                if p.inputs.is_empty() {
                    return None;
                }
                let source = &p.inputs[0];
                let ext = source.extension().and_then(|s| s.to_str()).unwrap_or("");
                let ext_with_dot = format!(".{}", ext);
                if cpp_extensions.contains(ext_with_dot.as_str()) {
                    let is_cpp = ext == "cc" || ext == "cpp" || ext == "cxx";
                    Some((p.id, source.clone(), is_cpp))
                } else {
                    None
                }
            })
            .collect();

        if products.is_empty() {
            return Ok(());
        }

        // Show progress bar for dependency scanning
        let pb = indicatif::ProgressBar::new(products.len() as u64);
        pb.set_style(
            indicatif::ProgressStyle::default_bar()
                .template("[cpp] Scanning dependencies {bar:40} {pos}/{len} {msg}")
                .unwrap()
                .progress_chars("##-")
        );

        for (id, source, is_cpp) in &products {
            pb.set_message(source.display().to_string());

            // Try to get cached dependencies, otherwise scan
            let headers = if let Some(cached) = deps_cache.get(source) {
                cached
            } else {
                let scanned = self.scan_dependencies(source, *is_cpp).unwrap_or_default();
                // Cache the result with analyzer tag (ignore errors)
                let _ = deps_cache.set(source, &scanned, "cpp");
                scanned
            };

            // Add header dependencies to the product
            if !headers.is_empty() {
                if let Some(product) = graph.get_product_mut(*id) {
                    // Avoid duplicates
                    for header in headers {
                        if !product.inputs.contains(&header) {
                            product.inputs.push(header);
                        }
                    }
                }
            }

            pb.inc(1);
        }
        pb.finish_and_clear();

        // Show cache stats
        let stats = deps_cache.stats();
        if stats.hits > 0 || stats.misses > 0 {
            eprintln!("[cpp] Dependency cache: {} hits, {} recalculated",
                stats.hits, stats.misses);
        }

        Ok(())
    }
}
