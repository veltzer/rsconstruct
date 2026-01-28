use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use walkdir::WalkDir;

use crate::config::CcConfig;
use crate::graph::{BuildGraph, Product};
use super::ProductDiscovery;

pub struct CcProcessor {
    project_root: PathBuf,
    config: CcConfig,
    source_dir: PathBuf,
    output_dir: PathBuf,
    deps_dir: PathBuf,
}

impl CcProcessor {
    pub fn new(project_root: PathBuf, config: CcConfig) -> Self {
        let source_dir = project_root.join(&config.source_dir);
        let output_dir = project_root.join("out/cc");
        let deps_dir = project_root.join(".rsb/deps");
        Self {
            project_root,
            config,
            source_dir,
            output_dir,
            deps_dir,
        }
    }

    /// Check if cc processing should be enabled
    fn should_process(&self) -> bool {
        self.source_dir.exists()
    }

    /// Find all C/C++ source files. Returns (path, is_cpp) pairs.
    fn find_source_files(&self) -> Vec<(PathBuf, bool)> {
        if !self.source_dir.exists() {
            return Vec::new();
        }

        WalkDir::new(&self.source_dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let path = e.path().to_path_buf();
                match path.extension().and_then(|s| s.to_str()) {
                    Some("c") => Some((path, false)),
                    Some("cc") => Some((path, true)),
                    _ => None,
                }
            })
            .collect()
    }

    /// Get executable path for a source file.
    /// Mirrors directory structure: src/a/b.c -> out/cc/a/b
    fn get_executable_path(&self, source: &Path) -> PathBuf {
        let relative = source
            .strip_prefix(&self.source_dir)
            .unwrap_or(source);
        let exe_name = relative.with_extension("");
        self.output_dir.join(exe_name)
    }

    /// Get deps file path for a source file.
    /// src/a/b.c -> .rsb/deps/a/b.c.d
    fn get_deps_path(&self, source: &Path) -> PathBuf {
        let relative = source
            .strip_prefix(&self.source_dir)
            .unwrap_or(source);
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

    /// Run gcc/g++ -MM to scan dependencies for a source file.
    /// Also writes the result to the deps cache.
    fn scan_dependencies(&self, source: &Path, is_cpp: bool) -> Result<Vec<PathBuf>> {
        let compiler = if is_cpp { &self.config.cxx } else { &self.config.cc };
        let flags = if is_cpp { &self.config.cxxflags } else { &self.config.cflags };

        let mut cmd = Command::new(compiler);
        cmd.arg("-MM");
        cmd.arg(format!("-I{}", self.source_dir.display()));
        for inc in &self.config.include_paths {
            cmd.arg(format!("-I{}", self.project_root.join(inc).display()));
        }
        for flag in flags {
            cmd.arg(flag);
        }
        cmd.arg(source);
        cmd.current_dir(&self.project_root);

        let output = cmd
            .output()
            .context(format!("Failed to run {} -MM on {}", compiler, source.display()))?;

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
                // Filter out system headers
                let path_str = path.to_string_lossy();
                if path_str.starts_with("/usr/") || path_str.starts_with("/lib/") {
                    return None;
                }
                // Convert to absolute path relative to project root
                let abs_path = if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    self.project_root.join(path)
                };
                Some(abs_path)
            })
            .collect()
    }

    /// Compile a single source file directly to an executable.
    fn compile_source(&self, source: &Path, executable: &Path, deps_file: &Path, is_cpp: bool) -> Result<()> {
        let compiler = if is_cpp { &self.config.cxx } else { &self.config.cc };
        let flags = if is_cpp { &self.config.cxxflags } else { &self.config.cflags };

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
        cmd.arg("-MMD");
        cmd.arg("-MF");
        cmd.arg(deps_file);
        cmd.arg(format!("-I{}", self.source_dir.display()));
        for inc in &self.config.include_paths {
            cmd.arg(format!("-I{}", self.project_root.join(inc).display()));
        }
        for flag in flags {
            cmd.arg(flag);
        }
        for flag in &self.config.ldflags {
            cmd.arg(flag);
        }
        cmd.arg("-o");
        cmd.arg(executable);
        cmd.arg(source);
        cmd.current_dir(&self.project_root);

        let output = cmd
            .output()
            .context(format!("Failed to run {} on {}", compiler, source.display()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Compilation failed for {}: {}", source.display(), stderr);
        }

        Ok(())
    }
}

impl ProductDiscovery for CcProcessor {
    fn discover(&self, graph: &mut BuildGraph) -> Result<()> {
        if !self.should_process() {
            return Ok(());
        }

        let source_files = self.find_source_files();
        if source_files.is_empty() {
            return Ok(());
        }

        for (source, is_cpp) in &source_files {
            let executable = self.get_executable_path(source);

            // Resolve header dependencies
            let headers = match self.read_cached_deps(source) {
                Some(h) => h,
                None => self.scan_dependencies(source, *is_cpp)
                    .unwrap_or_default(),
            };

            // Build inputs: source file + all headers
            let mut inputs = vec![source.clone()];
            inputs.extend(headers);

            graph.add_product(
                inputs,
                vec![executable],
                "cc",
            );
        }

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
        for output in &product.outputs {
            if output.exists() {
                fs::remove_file(output)?;
                println!("Removed: {}", output.display());
            }
        }
        Ok(())
    }
}
