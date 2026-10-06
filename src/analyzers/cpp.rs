//! C/C++ dependency analyzer for scanning header files.
//!
//! Scans source files for #include directives and adds header dependencies
//! to products in the build graph.

use anyhow::Result;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use crate::config::CppAnalyzerConfig;
use crate::file_index::FileIndex;
use crate::graph::Product;
use crate::processor::{check_command_output, format_command, run_command_capture};

use super::{DepAnalyzer, ScanResult};

const CPP_MATCH_EXTENSIONS: &[&str] = &["c", "cc", "cpp", "cxx"];

/// C/C++ dependency analyzer that scans source files for #include directives.
pub struct CppDepAnalyzer {
    config: CppAnalyzerConfig,
    verbose: bool,
    /// Cached canonical project root path (for stripping absolute prefixes from compiler output)
    canonical_root: OnceLock<PathBuf>,
    /// Cached include paths from pkg-config
    pkg_config_include_paths: OnceLock<Vec<PathBuf>>,
    /// Cached include paths from `include_path_commands`
    command_include_paths: OnceLock<Vec<PathBuf>>,
}

impl CppDepAnalyzer {
    pub const fn new(config: CppAnalyzerConfig, verbose: bool) -> Self {
        Self {
            config,
            verbose,
            canonical_root: OnceLock::new(),
            pkg_config_include_paths: OnceLock::new(),
            command_include_paths: OnceLock::new(),
        }
    }

    /// Get the canonical project root path (lazily computed).
    fn canonical_root(&self) -> &Path {
        self.canonical_root.get_or_init(|| {
            Path::new(".")
                .canonicalize()
                .unwrap_or_else(|_| PathBuf::from("."))
        })
    }

    /// Query pkg-config for include paths (lazy, cached).
    fn get_pkg_config_include_paths(&self, ctx: &crate::build_context::BuildContext) -> &[PathBuf] {
        self.pkg_config_include_paths.get_or_init(|| {
            super::query_pkg_config_include_paths(ctx, "cpp", &self.config.pkg_config, self.verbose)
        })
    }

    /// Run configured `include_path_commands` to get additional include paths (lazy, cached).
    fn get_command_include_paths(&self, ctx: &crate::build_context::BuildContext) -> &[PathBuf] {
        self.command_include_paths.get_or_init(|| {
            super::run_include_path_commands(
                ctx,
                "cpp",
                &self.config.include_path_commands,
                self.verbose,
            )
        })
    }

    /// Check if a path is within the project root (not a system header).
    /// A path that cannot be canonicalized is assumed not project-local.
    fn is_project_local(&self, path: &Path) -> bool {
        path.canonicalize()
            .is_ok_and(|canonical| canonical.starts_with(self.canonical_root()))
    }

    /// Check if a source path matches any of the configured exclude-dir segments.
    fn is_excluded(&self, path: &Path) -> bool {
        let path_str = path.to_string_lossy();
        self.config
            .src_exclude_dirs
            .iter()
            .any(|seg| path_str.contains(seg))
    }

    /// Run gcc/g++ -MM to scan dependencies for a source file.
    fn scan_dependencies_compiler(
        &self,
        ctx: &crate::build_context::BuildContext,
        source: &Path,
        is_cpp: bool,
    ) -> Result<Vec<PathBuf>> {
        let compiler = if is_cpp {
            &self.config.cxx
        } else {
            &self.config.cc
        };

        let mut cmd = Command::new(compiler);
        cmd.arg("-MM");

        // Add include paths
        for inc in &self.config.include_paths {
            cmd.arg(format!("-I{inc}"));
        }

        // Add pkg-config include paths
        for inc in self.get_pkg_config_include_paths(ctx) {
            cmd.arg(format!("-I{}", inc.display()));
        }

        // Add include paths from commands
        for inc in self.get_command_include_paths(ctx) {
            cmd.arg(format!("-I{}", inc.display()));
        }

        // Add compile flags
        let flags = if is_cpp {
            &self.config.cxxflags
        } else {
            &self.config.cflags
        };
        for flag in flags {
            cmd.arg(flag);
        }

        cmd.arg(source);

        if self.verbose {
            eprintln!("[cpp] {}", format_command(&cmd));
        }

        let output = run_command_capture(ctx, &cmd)?;
        check_command_output(
            &output,
            format_args!("Dependency scan of {}", source.display()),
        )?;

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
                            canonical
                                .strip_prefix(canonical_root)
                                .ok()
                                .map(std::path::Path::to_path_buf)
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

    /// The directories the compiler searches for a quoted include in
    /// `source`, in order: the source's own directory, then the `-I` paths
    /// in the order `scan_dependencies_compiler` passes them.
    fn search_dirs(&self, ctx: &crate::build_context::BuildContext, source: &Path) -> Vec<PathBuf> {
        std::iter::once(crate::processor::parent_dir_or_empty(source).to_path_buf())
            .chain(self.config.include_paths.iter().map(PathBuf::from))
            .chain(self.get_pkg_config_include_paths(ctx).iter().cloned())
            .chain(self.get_command_include_paths(ctx).iter().cloned())
            .map(|dir| without_cur_dir(&dir))
            .collect()
    }

    /// Scan dependencies using compiler -MM method.
    fn scan_dependencies(
        &self,
        ctx: &crate::build_context::BuildContext,
        source: &Path,
        is_cpp: bool,
    ) -> Result<Vec<PathBuf>> {
        self.scan_dependencies_compiler(ctx, source, is_cpp)
    }
}

impl DepAnalyzer for CppDepAnalyzer {
    fn description(&self) -> &'static str {
        "Scan C/C++ source files for #include dependencies"
    }

    fn enabled(&self) -> bool {
        self.config.enabled
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

    fn match_product(&self, p: &Product) -> Option<PathBuf> {
        if p.inputs.is_empty() {
            return None;
        }
        let source = &p.inputs[0];
        if self.is_excluded(source) {
            return None;
        }
        let ext = source.extension().and_then(|s| s.to_str()).unwrap_or("");
        if CPP_MATCH_EXTENSIONS.contains(&ext) {
            Some(source.clone())
        } else {
            None
        }
    }

    fn scan(
        &self,
        ctx: &crate::build_context::BuildContext,
        source: &Path,
        _file_index: &FileIndex,
    ) -> Result<ScanResult> {
        let ext = source.extension().and_then(|s| s.to_str()).unwrap_or("");
        let is_cpp = ext == "cc" || ext == "cpp" || ext == "cxx";
        let deps = self.scan_dependencies(ctx, source, is_cpp)?;
        let absent = shadow_probes(&self.search_dirs(ctx, source), &deps);
        Ok(ScanResult::deps(deps, absent))
    }

    fn fingerprint_parts(&self, ctx: &crate::build_context::BuildContext) -> Result<Vec<String>> {
        let mut parts = super::config_fingerprint(&self.config)?;
        parts.extend(
            self.get_pkg_config_include_paths(ctx)
                .iter()
                .chain(self.get_command_include_paths(ctx))
                .map(|p| p.display().to_string()),
        );
        Ok(parts)
    }
}

/// The paths the compiler probed, and found empty, before each of `deps`.
///
/// `-MM` names the header it found, not how it was spelled in the
/// `#include`, so the spelling is reconstructed: for every search directory
/// a header lies under, its path relative to that directory is a possible
/// spelling, and that spelling under each directory searched *earlier* is a
/// probe that came up empty — a file appearing there would now be found
/// instead. Covering every possible spelling over-approximates (a
/// superfluous probe can only cause a needless rescan), and the includer's
/// own directory is approximated by the source's. Paths that exist are left
/// out: they were not what the compiler found, so they were never probed.
fn shadow_probes(search_dirs: &[PathBuf], deps: &[PathBuf]) -> Vec<PathBuf> {
    let mut probes: Vec<PathBuf> = Vec::new();
    for dep in deps {
        for (found_at, dir) in search_dirs.iter().enumerate() {
            let Ok(spelling) = dep.strip_prefix(dir) else {
                continue;
            };
            for earlier in &search_dirs[..found_at] {
                let probe = earlier.join(spelling);
                if !probe.exists() && !probes.contains(&probe) {
                    probes.push(probe);
                }
            }
        }
    }
    probes
}

/// `dir` without `./` components, so `./include` and `include` both prefix
/// the relative paths `-MM` reports.
fn without_cur_dir(dir: &Path) -> PathBuf {
    dir.components()
        .filter(|c| !matches!(c, std::path::Component::CurDir))
        .collect()
}

inventory::submit! {
    crate::registries::AnalyzerPlugin {
        name: "cpp",
        description: "Scan C/C++ source files for #include dependencies (using compiler -MM)",
        is_native: false,
        create: |toml_value, verbose| {
            let cfg: CppAnalyzerConfig = toml::from_str(&toml::to_string(toml_value)?)?;
            Ok(Box::new(CppDepAnalyzer::new(cfg, verbose)))
        },
        defconfig_toml: || {
            toml::to_string_pretty(&crate::config::CppAnalyzerConfig::default()).ok()
        },
        known_fields: crate::registries::typed_known_fields::<crate::config::CppAnalyzerConfig>,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `include/x.h` found through `-Iinclude` was spelled `x.h`, so the
    /// compiler first tried `src/x.h`; spelled `include/x.h` it would be
    /// found from the source directory first, which probes nothing. Both
    /// spellings are covered. Paths that exist are never probes.
    #[test]
    fn shadow_probes_cover_every_spelling_before_the_hit() {
        let tmp = tempfile::TempDir::new().unwrap();
        let src = tmp.path().join("src");
        let include = tmp.path().join("include");
        let other = tmp.path().join("other");
        fs_create(&[&src, &include, &other]);
        let header = include.join("x.h");
        std::fs::write(&header, "").unwrap();
        std::fs::write(other.join("x.h"), "").unwrap();

        let dirs = vec![src.clone(), include, other];
        let probes = shadow_probes(&dirs, &[header]);
        assert_eq!(probes, vec![src.join("x.h")]);
    }

    #[test]
    fn without_cur_dir_strips_dot_components() {
        assert_eq!(
            without_cur_dir(Path::new("./include")),
            PathBuf::from("include")
        );
        assert_eq!(without_cur_dir(Path::new(".")), PathBuf::new());
    }

    fn fs_create(dirs: &[&Path]) {
        for dir in dirs {
            std::fs::create_dir_all(dir).unwrap();
        }
    }
}
