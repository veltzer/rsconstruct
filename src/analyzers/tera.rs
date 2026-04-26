//! Tera template dependency analyzer for scanning include and import directives.
//!
//! Scans Tera template files for `{% include %}`, `{% import %}`, and `{% extends %}`
//! directives and adds referenced template files as dependencies to products in the build graph.

use anyhow::{Result, bail};
use regex::Regex;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::config::TeraAnalyzerConfig;
use crate::deps_cache::DepsCache;
use crate::errors;
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};

use super::{DepAnalyzer, ScanResult};

use indicatif::ProgressBar;

/// Tera template dependency analyzer that scans for include/import/extends directives.
pub struct TeraDepAnalyzer {
    iname: String,
    config: TeraAnalyzerConfig,
}

impl TeraDepAnalyzer {
    pub fn new(iname: &str, config: TeraAnalyzerConfig) -> Self {
        Self { iname: iname.to_string(), config }
    }

    /// Scan a Tera template file for all dependency-affecting constructs.
    /// Returns the resolved file paths (added to product.inputs) and a config-hash
    /// contribution that captures non-content state — the sorted set of paths
    /// matching each glob, plus the literal text of each shell command.
    fn scan_template(&self, source: &Path) -> Result<ScanResult> {
        let content = crate::errors::ctx(fs::read_to_string(source), &format!("Failed to read template: {}", source.display()))?;
        let mut paths: Vec<PathBuf> = Vec::new();
        let mut seen: HashSet<PathBuf> = HashSet::new();
        // Pieces accumulated into the config_hash: sorted paths from each glob,
        // plus literal command strings. Order matters and is determined by the
        // order in which they appear in the template, which is stable.
        let mut hash_pieces: Vec<String> = Vec::new();

        // {% include "path" %}, {% import "path" %}, {% extends "path" %}
        // Single quotes also handled.
        static INCLUDE_RE: OnceLock<Regex> = OnceLock::new();
        let include_re = INCLUDE_RE.get_or_init(|| {
            Regex::new(r#"\{%[-~]?\s*(?:include|import|extends)\s+["']([^"']+)["']"#)
                .expect(errors::INVALID_REGEX)
        });

        // load_lua/load_data/load_json/load_toml/load_csv(path="...")
        static LOAD_RE: OnceLock<Regex> = OnceLock::new();
        let load_re = LOAD_RE.get_or_init(|| {
            Regex::new(r#"load_(?:lua|data|json|toml|csv)\s*\(\s*path\s*=\s*["']([^"']+)["']"#)
                .expect(errors::INVALID_REGEX)
        });

        // glob(pattern="...") — first-class directory query.
        static GLOB_RE: OnceLock<Regex> = OnceLock::new();
        let glob_re = GLOB_RE.get_or_init(|| {
            Regex::new(r#"glob\s*\(\s*pattern\s*=\s*["']([^"']+)["']\s*\)"#)
                .expect(errors::INVALID_REGEX)
        });

        // shell_output(...) — full call. We pull out the command and depends_on
        // separately. The full body capture is intentionally lazy; a missing
        // depends_on must be diagnosed (analyzer-time error).
        static SHELL_OUTPUT_RE: OnceLock<Regex> = OnceLock::new();
        let shell_re = SHELL_OUTPUT_RE.get_or_init(|| {
            // Match `shell_output(... )` — body is everything up to the matching close paren.
            // Tera template syntax doesn't nest calls of shell_output inside itself, so a
            // non-greedy match up to the next `)` is sufficient in practice. False positives
            // (e.g. `)` inside a quoted command string) are rare; the inner extraction below
            // handles malformed bodies by simply not matching.
            Regex::new(r#"shell_output\s*\(([^)]*)\)"#)
                .expect(errors::INVALID_REGEX)
        });

        // Inner extraction inside a shell_output(...) body: `command="..."` and `depends_on=[...]`.
        static SHELL_CMD_RE: OnceLock<Regex> = OnceLock::new();
        let shell_cmd_re = SHELL_CMD_RE.get_or_init(|| {
            Regex::new(r#"command\s*=\s*["']([^"']*)["']"#).expect(errors::INVALID_REGEX)
        });
        static SHELL_DEPS_RE: OnceLock<Regex> = OnceLock::new();
        let shell_deps_re = SHELL_DEPS_RE.get_or_init(|| {
            Regex::new(r#"depends_on\s*=\s*\[([^\]]*)\]"#).expect(errors::INVALID_REGEX)
        });
        static QUOTED_STR_RE: OnceLock<Regex> = OnceLock::new();
        let quoted_str_re = QUOTED_STR_RE.get_or_init(|| {
            Regex::new(r#"["']([^"']+)["']"#).expect(errors::INVALID_REGEX)
        });

        let source_dir = source.parent().unwrap_or(Path::new("."));

        // 1) include/import/extends and load_*
        for caps in include_re.captures_iter(&content).chain(load_re.captures_iter(&content)) {
            let path_str = &caps[1];
            if path_str.is_empty() {
                continue;
            }
            let candidates = [
                source_dir.join(path_str),
                PathBuf::from(path_str),
            ];
            for candidate in &candidates {
                if candidate.is_file() && !seen.contains(candidate) {
                    seen.insert(candidate.clone());
                    paths.push(candidate.clone());
                    break;
                }
            }
        }

        // 2) glob(pattern="...")
        for caps in glob_re.captures_iter(&content) {
            let pattern = &caps[1];
            let matched = expand_glob(pattern)?;
            // Mix literal pattern + sorted resolved paths into the hash.
            // The literal pattern is included so that "marp/**/*.md" with zero
            // matches and "marp/**/*.txt" with zero matches don't collide.
            hash_pieces.push(format!("glob:{}", pattern));
            hash_pieces.push(format!("glob_resolved:{}", matched.join("\n")));
            for p in matched {
                let pb = PathBuf::from(p);
                if !seen.contains(&pb) {
                    seen.insert(pb.clone());
                    paths.push(pb);
                }
            }
        }

        // 3) shell_output(...): require depends_on, harvest patterns and command
        for caps in shell_re.captures_iter(&content) {
            let body = &caps[1];
            let command = shell_cmd_re.captures(body)
                .and_then(|c| c.get(1).map(|m| m.as_str().to_string()));
            let deps_block = shell_deps_re.captures(body)
                .and_then(|c| c.get(1).map(|m| m.as_str().to_string()));

            let Some(command) = command else {
                bail!(
                    "[tera] {}: shell_output(...) call has no command= argument. \
                     Found: shell_output({})",
                    source.display(), body.trim(),
                );
            };
            let Some(deps_block) = deps_block else {
                bail!(
                    "[tera] {}: shell_output(command=\"{}\") is missing depends_on=[...].\n\
                     rsconstruct cannot otherwise tell when its output should be invalidated.\n\
                     Migrate to glob(pattern=\"...\") for directory queries, or pass an explicit \
                     list (e.g. depends_on=[\"marp/**/*.md\"]).\n\
                     If your command genuinely has no file dependencies, pass depends_on=[] \
                     to acknowledge that.",
                    source.display(), command,
                );
            };

            // Mix the command into the hash so editing it triggers rebuild.
            hash_pieces.push(format!("shell_cmd:{}", command));

            // Parse the depends_on list (sequence of quoted strings).
            let mut patterns: Vec<String> = Vec::new();
            for pcap in quoted_str_re.captures_iter(&deps_block) {
                patterns.push(pcap[1].to_string());
            }
            if patterns.is_empty() {
                // Empty list — explicit user acknowledgement that the command
                // depends on nothing rsconstruct can track. Mix nothing extra;
                // the command string itself is already in the hash.
                hash_pieces.push("shell_deps:[]".to_string());
                continue;
            }
            for pattern in &patterns {
                let matched = expand_glob(pattern)?;
                hash_pieces.push(format!("shell_dep:{}", pattern));
                hash_pieces.push(format!("shell_dep_resolved:{}", matched.join("\n")));
                for p in matched {
                    let pb = PathBuf::from(p);
                    if !seen.contains(&pb) {
                        seen.insert(pb.clone());
                        paths.push(pb);
                    }
                }
            }
        }

        let config_hash_contribution = if hash_pieces.is_empty() {
            None
        } else {
            Some(hash_pieces.join("|"))
        };

        Ok(ScanResult {
            deps: paths,
            config_hash_contribution,
        })
    }
}

/// Expand a glob pattern into a sorted list of file paths (as strings, relative
/// to project root). Symlinks and directories are skipped — only regular files
/// participate. The sorted order matters for the deterministic hash piece.
fn expand_glob(pattern: &str) -> Result<Vec<String>> {
    let mut paths: Vec<String> = Vec::new();
    for entry in glob::glob(pattern)
        .map_err(|e| anyhow::anyhow!("Invalid glob pattern '{}': {}", pattern, e))?
    {
        let path = entry
            .map_err(|e| anyhow::anyhow!("Glob iteration error for '{}': {}", pattern, e))?;
        if path.is_file() {
            paths.push(path.to_string_lossy().into_owned());
        }
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

impl DepAnalyzer for TeraDepAnalyzer {
    fn description(&self) -> &str {
        "Scan Tera templates for include/import/extends dependencies"
    }

    fn enabled(&self) -> bool {
        self.config.enabled
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        file_index.has_extension(".tera")
    }

    fn match_product(&self, p: &Product) -> Option<PathBuf> {
        if p.inputs.is_empty() {
            return None;
        }
        let source = &p.inputs[0];
        let ext = source.extension().and_then(|s| s.to_str()).unwrap_or("");
        if ext == "tera" { Some(source.clone()) } else { None }
    }

    fn analyze(
        &self,
        ctx: &crate::build_context::BuildContext,
        graph: &mut BuildGraph,
        deps_cache: &mut DepsCache,
        _file_index: &FileIndex,
        _verbose: bool,
        progress: &ProgressBar,
    ) -> Result<()> {
        super::analyze_with_full_scanner(
            ctx,
            graph,
            deps_cache,
            &self.iname,
            |p| self.match_product(p),
            |source| self.scan_template(source),
            progress,
        )
    }
}

inventory::submit! {
    crate::registries::AnalyzerPlugin {
        name: "tera",
        description: "Scan Tera templates for include/import/extends dependencies",
        is_native: true,
        create: |iname, toml_value, _| {
            let cfg: TeraAnalyzerConfig = toml::from_str(&toml::to_string(toml_value)?)?;
            Ok(Box::new(TeraDepAnalyzer::new(iname, cfg)))
        },
        defconfig_toml: || {
            toml::to_string_pretty(&TeraAnalyzerConfig::default()).ok()
        },
        known_fields: crate::registries::typed_known_fields::<TeraAnalyzerConfig>,
    }
}
