//! Tera template dependency analyzer for scanning include and import directives.
//!
//! Scans Tera template files for `{% include %}`, `{% import %}`, and `{% extends %}`
//! directives and adds referenced template files as dependencies to products in the build graph.

use anyhow::Result;
use regex::Regex;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::deps_cache::DepsCache;
use crate::errors;
use crate::file_index::FileIndex;
use crate::graph::BuildGraph;

use super::DepAnalyzer;

/// Tera template dependency analyzer that scans for include/import/extends directives.
pub struct TeraDepAnalyzer;

impl TeraDepAnalyzer {
    pub fn new() -> Self {
        Self
    }

    /// Scan a Tera template file for include, import, and extends references.
    /// Returns paths to local template files that are referenced.
    fn scan_includes(&self, source: &Path) -> Result<Vec<PathBuf>> {
        let content = fs::read_to_string(source)?;
        let mut includes = Vec::new();
        let mut seen = HashSet::new();

        // Match {% include "path" %}, {% import "path" %}, {% extends "path" %}
        // Also handles single quotes: {% include 'path' %}
        static INCLUDE_RE: OnceLock<Regex> = OnceLock::new();
        let include_re = INCLUDE_RE.get_or_init(|| {
            Regex::new(r#"\{%[-~]?\s*(?:include|import|extends)\s+["']([^"']+)["']"#)
                .expect(errors::INVALID_REGEX)
        });

        // Match load_lua(path="file"), load_data(path="file"), etc.
        // These are Tera function calls that load external files.
        static LOAD_RE: OnceLock<Regex> = OnceLock::new();
        let load_re = LOAD_RE.get_or_init(|| {
            Regex::new(r#"load_(?:lua|data|json|toml|csv)\s*\(\s*path\s*=\s*["']([^"']+)["']"#)
                .expect(errors::INVALID_REGEX)
        });

        let source_dir = source.parent().unwrap_or(Path::new("."));

        // Collect paths from both regex patterns
        let all_captures = include_re.captures_iter(&content)
            .chain(load_re.captures_iter(&content));

        for caps in all_captures {
            let path_str = &caps[1];

            if path_str.is_empty() {
                continue;
            }

            // Try resolving relative to the source file's directory first,
            // then relative to project root
            let candidates = [
                source_dir.join(path_str),
                PathBuf::from(path_str),
            ];

            for candidate in &candidates {
                if candidate.is_file() && !seen.contains(candidate) {
                    seen.insert(candidate.clone());
                    includes.push(candidate.clone());
                    break;
                }
            }
        }

        Ok(includes)
    }
}

impl DepAnalyzer for TeraDepAnalyzer {
    fn description(&self) -> &str {
        "Scan Tera templates for include/import/extends dependencies"
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        file_index.has_extension(".tera")
    }

    fn analyze(&self, graph: &mut BuildGraph, deps_cache: &mut DepsCache, _file_index: &FileIndex, verbose: bool) -> Result<()> {
        super::analyze_with_scanner(
            graph,
            deps_cache,
            "tera",
            |p| {
                if p.inputs.is_empty() {
                    return None;
                }
                let source = &p.inputs[0];
                let ext = source.extension().and_then(|s| s.to_str()).unwrap_or("");
                if ext == "tera" {
                    Some(source.clone())
                } else {
                    None
                }
            },
            |source| self.scan_includes(source),
            verbose,
        )
    }
}
