//! Markdown dependency analyzer for scanning image and file references.
//!
//! Scans Markdown source files for image references (`![alt](path)`) and
//! adds referenced local files as dependencies to products in the build graph.

use anyhow::Result;
use regex::Regex;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::config::MarkdownAnalyzerConfig;
use crate::errors;
use crate::file_index::FileIndex;
use crate::graph::Product;

use super::{DepAnalyzer, ScanResult};

/// Markdown dependency analyzer that scans source files for image and link references.
pub struct MarkdownDepAnalyzer {
    config: MarkdownAnalyzerConfig,
}

impl MarkdownDepAnalyzer {
    pub const fn new(config: MarkdownAnalyzerConfig) -> Self {
        Self { config }
    }

    /// Scan a Markdown file for local file references.
    /// Returns paths to local files referenced via `![alt](path)` or
    /// `[text](path)` syntax, and the candidate paths probed and found
    /// missing — a reference to a file that does not exist yet must be
    /// picked up once it does. A reference resolves to a file on disk or to
    /// one another product will generate (in `file_index`).
    fn scan_references(&self, source: &Path, file_index: &FileIndex) -> Result<ScanResult> {
        let content = crate::errors::ctx(
            fs::read_to_string(source),
            &format!("Failed to read markdown: {}", source.display()),
        )?;
        let mut refs = Vec::new();
        let mut absent = Vec::new();
        let mut seen = HashSet::new();

        // Match ![alt](path) and [text](path) — capture the path portion
        // Excludes URLs (http://, https://, ftp://, data:, #anchors)
        static REF_RE: OnceLock<Regex> = OnceLock::new();
        let ref_re = REF_RE.get_or_init(|| {
            Regex::new(r"!?\[(?:[^\]]*)\]\(([^)]+)\)").expect(errors::INVALID_REGEX)
        });

        let source_dir = crate::processor::parent_dir(source);

        for caps in ref_re.captures_iter(&content) {
            let path_str = caps[1].trim();

            // Skip URLs, anchors, and data URIs
            if path_str.starts_with("http://")
                || path_str.starts_with("https://")
                || path_str.starts_with("ftp://")
                || path_str.starts_with("data:")
                || path_str.starts_with('#')
            {
                continue;
            }

            // Strip optional title: ![alt](path "title")
            let path_str = path_str.split_whitespace().next().unwrap_or(path_str);
            // Strip anchor fragments: path#section
            let path_str = path_str.split('#').next().unwrap_or(path_str);

            if path_str.is_empty() {
                continue;
            }

            // Try resolving relative to the source file's directory first,
            // then relative to the project root (cwd)
            let candidates = [source_dir.join(path_str), PathBuf::from(path_str)];
            let found = candidates
                .iter()
                .find(|c| c.is_file() || file_index.contains(c));
            absent.extend(super::probed_before(&candidates, found));
            if let Some(found) = found
                && seen.insert(found.clone())
            {
                refs.push(found.clone());
            }
        }
        absent.sort();
        absent.dedup();

        Ok(ScanResult::deps(refs, absent))
    }
}

impl DepAnalyzer for MarkdownDepAnalyzer {
    fn description(&self) -> &'static str {
        "Scan Markdown files for local file dependencies"
    }

    fn enabled(&self) -> bool {
        self.config.enabled
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        file_index.has_extension(".md")
    }

    fn match_product(&self, p: &Product) -> Option<PathBuf> {
        if p.inputs.is_empty() {
            return None;
        }
        let source = &p.inputs[0];
        let ext = source.extension().and_then(|s| s.to_str()).unwrap_or("");
        if ext == "md" {
            Some(source.clone())
        } else {
            None
        }
    }

    fn scan(
        &self,
        _ctx: &crate::build_context::BuildContext,
        source: &Path,
        file_index: &FileIndex,
    ) -> Result<ScanResult> {
        self.scan_references(source, file_index)
    }

    fn fingerprint_parts(&self, _ctx: &crate::build_context::BuildContext) -> Result<Vec<String>> {
        super::config_fingerprint(&self.config)
    }
}

inventory::submit! {
    crate::registries::AnalyzerPlugin {
        name: "markdown",
        description: "Scan Markdown files for local file dependencies",
        is_native: true,
        create: |toml_value, _| {
            let cfg: MarkdownAnalyzerConfig = toml::from_str(&toml::to_string(toml_value)?)?;
            Ok(Box::new(MarkdownDepAnalyzer::new(cfg)))
        },
        defconfig_toml: || {
            toml::to_string_pretty(&MarkdownAnalyzerConfig::default()).ok()
        },
        known_fields: crate::registries::typed_known_fields::<MarkdownAnalyzerConfig>,
    }
}
