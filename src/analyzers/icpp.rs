//! In-process C/C++ dependency analyzer (`icpp`).
//!
//! Uses a pure-Rust regex scanner to find `#include` directives — no external tools.
//! For projects that need compiler-accurate scanning (macros, conditional includes),
//! use the `cpp` analyzer instead.

use anyhow::Result;
use regex::Regex;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::config::IcppAnalyzerConfig;
use crate::errors;
use crate::file_index::FileIndex;
use crate::graph::Product;

use super::{DepAnalyzer, ScanResult};

const CPP_MATCH_EXTENSIONS: &[&str] = &["c", "cc", "cpp", "cxx"];

/// In-process C/C++ dependency analyzer using a pure-Rust regex scanner.
pub struct IcppDepAnalyzer {
    config: IcppAnalyzerConfig,
    verbose: bool,
    /// Cached include paths discovered from pkg-config
    pkg_config_include_paths: OnceLock<Vec<PathBuf>>,
    /// Cached include paths from `include_path_commands`
    command_include_paths: OnceLock<Vec<PathBuf>>,
}

impl IcppDepAnalyzer {
    pub const fn new(config: IcppAnalyzerConfig, verbose: bool) -> Self {
        Self {
            config,
            verbose,
            pkg_config_include_paths: OnceLock::new(),
            command_include_paths: OnceLock::new(),
        }
    }

    fn is_excluded(&self, path: &Path) -> bool {
        let path_str = path.to_string_lossy();
        self.config
            .src_exclude_dirs
            .iter()
            .any(|seg| path_str.contains(seg))
    }

    /// Query pkg-config for include paths (lazy, cached).
    fn get_pkg_config_include_paths(
        &self,
        ctx: &crate::build_context::BuildContext,
    ) -> Result<&[PathBuf]> {
        super::cached_include_paths(&self.pkg_config_include_paths, || {
            super::query_pkg_config_include_paths(
                ctx,
                "icpp",
                &self.config.pkg_config,
                self.verbose,
            )
        })
    }

    /// Run configured `include_path_commands` to get additional include paths (lazy, cached).
    fn get_command_include_paths(
        &self,
        ctx: &crate::build_context::BuildContext,
    ) -> Result<&[PathBuf]> {
        super::cached_include_paths(&self.command_include_paths, || {
            super::run_include_path_commands(
                ctx,
                "icpp",
                &self.config.include_path_commands,
                self.verbose,
            )
        })
    }

    /// Resolve a single `#include` directive to a file, if any, along with
    /// the candidates probed and found missing before it.
    /// Searches in order: including file's directory, configured `include_paths`,
    /// pkg-config-discovered include paths, then include paths from configured commands.
    /// A candidate matches when it is on disk or another product generates
    /// it (`file_index` holds declared outputs).
    fn resolve_include(
        &self,
        ctx: &crate::build_context::BuildContext,
        include: &str,
        including_dir: &Path,
        file_index: &FileIndex,
    ) -> Result<(Option<PathBuf>, Vec<PathBuf>)> {
        let candidates: Vec<PathBuf> = std::iter::once(including_dir.join(include))
            .chain(
                self.config
                    .include_paths
                    .iter()
                    .map(|dir| Path::new(dir).join(include)),
            )
            .chain(
                self.get_pkg_config_include_paths(ctx)?
                    .iter()
                    .map(|dir| dir.join(include)),
            )
            .chain(
                self.get_command_include_paths(ctx)?
                    .iter()
                    .map(|dir| dir.join(include)),
            )
            .collect();
        let found = candidates
            .iter()
            .find(|c| c.is_file() || file_index.contains(c))
            .cloned();
        let absent = super::probed_before(&candidates, found.as_ref());
        Ok((found, absent))
    }

    /// Scan a single file for `#include` directives. Returns resolved dep
    /// paths and the paths probed and found missing (see `ScanResult`).
    /// Errors if a `"quoted"` include can't be resolved (system headers via `<angle>`
    /// are allowed to be unresolved — they may live in system include paths).
    fn scan_file_includes(
        &self,
        ctx: &crate::build_context::BuildContext,
        source: &Path,
        file_index: &FileIndex,
    ) -> Result<(Vec<PathBuf>, Vec<PathBuf>)> {
        let content = errors::ctx(
            fs::read_to_string(source),
            &format!("Failed to read {}", source.display()),
        )?;

        static INCLUDE_RE: OnceLock<Regex> = OnceLock::new();
        let re = INCLUDE_RE.get_or_init(|| {
            // Capture group 1: opening delimiter ("" or "<"), group 2: include path
            Regex::new(r#"^\s*#\s*include\s*(["<])([^>"]+)[>"]"#).expect(errors::INVALID_REGEX)
        });

        let parent = crate::processor::parent_dir_or_empty(source);
        let mut deps = Vec::new();
        let mut absent = Vec::new();
        for line in content.lines() {
            if let Some(caps) = re.captures(line) {
                let is_quoted = &caps[1] == "\"";
                let include = &caps[2];
                if !is_quoted && !self.config.follow_angle_brackets {
                    continue;
                }
                let (found, probed) = self.resolve_include(ctx, include, parent, file_index)?;
                absent.extend(probed);
                match found {
                    Some(resolved) => deps.push(resolved),
                    None if is_quoted && !self.config.skip_not_found => {
                        anyhow::bail!(
                            "Include not found: #include \"{}\" in {}",
                            include,
                            source.display()
                        );
                    }
                    None => {}
                }
            }
        }
        Ok((deps, absent))
    }

    /// Recursively scan `source` for transitive includes: the full set of
    /// project-local header files it depends on (excluding the source
    /// itself), and every path probed and found missing along the way.
    /// A header another product has yet to generate is a dependency but
    /// cannot be read yet; the source is analyzed again once it is written
    /// (`Analysis::reanalyze`).
    /// Propagates errors from `scan_file_includes` (including "Include not found").
    fn scan_includes(
        &self,
        ctx: &crate::build_context::BuildContext,
        source: &Path,
        file_index: &FileIndex,
    ) -> Result<ScanResult> {
        let mut seen: HashSet<PathBuf> = HashSet::new();
        let mut headers: Vec<PathBuf> = Vec::new();
        let mut absent: Vec<PathBuf> = Vec::new();
        let mut queue: Vec<PathBuf> = vec![source.to_path_buf()];

        while let Some(file) = queue.pop() {
            let (direct_deps, probed) = self.scan_file_includes(ctx, &file, file_index)?;
            absent.extend(probed);
            for dep in direct_deps {
                if seen.insert(dep.clone()) {
                    headers.push(dep.clone());
                    if dep.exists() {
                        queue.push(dep);
                    }
                }
            }
        }
        absent.sort();
        absent.dedup();

        Ok(ScanResult::deps(headers, absent))
    }
}

impl DepAnalyzer for IcppDepAnalyzer {
    fn description(&self) -> &'static str {
        "Scan C/C++ source files for #include dependencies (in-process, regex-based)"
    }

    fn enabled(&self) -> bool {
        self.config.enabled
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
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
        file_index: &FileIndex,
    ) -> Result<ScanResult> {
        self.scan_includes(ctx, source, file_index)
    }

    /// Resolves pkg-config and the include-path commands, so a failure in
    /// either stops the build before any file is scanned.
    fn fingerprint_parts(&self, ctx: &crate::build_context::BuildContext) -> Result<Vec<String>> {
        let mut parts = super::config_fingerprint(&self.config)?;
        parts.extend(
            self.get_pkg_config_include_paths(ctx)?
                .iter()
                .chain(self.get_command_include_paths(ctx)?)
                .map(|p| p.display().to_string()),
        );
        Ok(parts)
    }
}

inventory::submit! {
    crate::registries::AnalyzerPlugin {
        name: "icpp",
        description: "Scan C/C++ source files for #include dependencies (in-process, regex-based)",
        is_native: true,
        create: |toml_value, verbose| {
            let cfg: IcppAnalyzerConfig = toml::from_str(&toml::to_string(toml_value)?)?;
            Ok(Box::new(IcppDepAnalyzer::new(cfg, verbose)))
        },
        defconfig_toml: || {
            toml::to_string_pretty(&crate::config::IcppAnalyzerConfig::default()).ok()
        },
        known_fields: crate::registries::typed_known_fields::<crate::config::IcppAnalyzerConfig>,
    }
}
