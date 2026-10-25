use anyhow::{Context, Result};
use parking_lot::Mutex;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::{
    OnceTool, PlannedOutput, add_planned_products, files_under, fresh_state_dir, install_staged,
    relative_slash_path, require_planned_output,
};
use crate::config::{StandardConfig, output_config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processor::{
    Processor, anchor_display_dir, check_command_output, resolve_anchor_path, run_in_anchor_dir,
};

const CORE_STATIC_FILES: &[&str] = &[
    ".nojekyll",
    "404.html",
    "ayu-highlight.css",
    "book.js",
    "clipboard.min.js",
    "css/chrome.css",
    "css/general.css",
    "css/print.css",
    "css/variables.css",
    "favicon.png",
    "favicon.svg",
    "highlight.css",
    "highlight.js",
    "index.html",
    "toc.html",
    "toc.js",
    "tomorrow-night.css",
];

const SEARCH_STATIC_FILES: &[&str] = &[
    "elasticlunr.min.js",
    "mark.min.js",
    "searcher.js",
    "searchindex.js",
];

const FONT_STATIC_FILES: &[&str] = &[
    "fonts/OPEN-SANS-LICENSE.txt",
    "fonts/SOURCE-CODE-PRO-LICENSE.txt",
    "fonts/fonts.css",
    "fonts/open-sans-v17-all-charsets-300.woff2",
    "fonts/open-sans-v17-all-charsets-300italic.woff2",
    "fonts/open-sans-v17-all-charsets-600.woff2",
    "fonts/open-sans-v17-all-charsets-600italic.woff2",
    "fonts/open-sans-v17-all-charsets-700.woff2",
    "fonts/open-sans-v17-all-charsets-700italic.woff2",
    "fonts/open-sans-v17-all-charsets-800.woff2",
    "fonts/open-sans-v17-all-charsets-800italic.woff2",
    "fonts/open-sans-v17-all-charsets-italic.woff2",
    "fonts/open-sans-v17-all-charsets-regular.woff2",
    "fonts/source-code-pro-v11-all-charsets-500.woff2",
];

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
/// Mdbook config. Custom: `loose_manifest`.
pub struct MdbookConfig {
    /// Report plan-vs-build mismatches as warnings instead of errors.
    #[serde(default)]
    pub loose_manifest: bool,
    #[serde(flatten)]
    pub standard: StandardConfig,
}

struct BookPlan {
    output_dir: PathBuf,
    outputs: Vec<PlannedOutput>,
    relative: BTreeSet<String>,
}

pub struct MdbookProcessor {
    config: MdbookConfig,
    runs: Mutex<BTreeMap<PathBuf, Arc<OnceTool>>>,
}

impl MdbookProcessor {
    pub const fn new(config: MdbookConfig) -> Self {
        Self {
            config,
            runs: Mutex::new(BTreeMap::new()),
        }
    }

    fn run_guard(&self, book_toml: &Path) -> Arc<OnceTool> {
        let mut guard = self.runs.lock();
        Arc::clone(
            guard
                .entry(book_toml.to_path_buf())
                .or_insert_with(|| Arc::new(OnceTool::new())),
        )
    }

    /// Plan all files `mdbook build` writes for `book_toml`.
    fn plan_book(&self, book_toml: &Path) -> Result<BookPlan> {
        let text = fs::read_to_string(book_toml)
            .with_context(|| format!("Failed to read {}", book_toml.display()))?;
        let doc: toml::Value = toml::from_str(&text)
            .with_context(|| format!("Failed to parse {}", book_toml.display()))?;

        let anchor_dir = crate::processor::parent_dir_or_empty(book_toml);
        let src_rel = doc
            .get("book")
            .and_then(|b| b.get("src"))
            .and_then(toml::Value::as_str)
            .unwrap_or("src");
        let src_dir = resolve_anchor_path(anchor_dir, src_rel);
        let output_dir = resolve_anchor_path(anchor_dir, &self.config.standard.output_dir);

        let html = doc.get("output").and_then(|o| o.get("html"));
        if html
            .and_then(|h| h.get("hash-files"))
            .and_then(toml::Value::as_bool)
            == Some(true)
        {
            anyhow::bail!(
                "{}: [output.html].hash-files = true is not supported by the mdbook mass generator; omit it so mdbook writes deterministic filenames",
                book_toml.display()
            );
        }
        let print_enabled = html
            .and_then(|h| h.get("print"))
            .and_then(|p| p.get("enable"))
            .and_then(toml::Value::as_bool)
            .unwrap_or(true);
        let search_enabled = html
            .and_then(|h| h.get("search"))
            .and_then(|s| s.get("enable"))
            .and_then(toml::Value::as_bool)
            .unwrap_or(true);
        let copy_fonts = html
            .and_then(|h| h.get("copy-fonts"))
            .and_then(toml::Value::as_bool)
            .unwrap_or(true);

        let mut relative: BTreeSet<String> =
            CORE_STATIC_FILES.iter().map(|s| (*s).to_string()).collect();
        if print_enabled {
            relative.insert("print.html".to_string());
        }
        if search_enabled {
            for f in SEARCH_STATIC_FILES {
                relative.insert((*f).to_string());
            }
        }
        if copy_fonts {
            for f in FONT_STATIC_FILES {
                relative.insert((*f).to_string());
            }
        }

        let mut sources = vec![book_toml.to_path_buf()];
        for key in ["additional-css", "additional-js"] {
            if let Some(arr) = html
                .and_then(|h| h.get(key))
                .and_then(toml::Value::as_array)
            {
                for item in arr.iter().filter_map(toml::Value::as_str) {
                    let trimmed = item.strip_prefix("./").unwrap_or(item);
                    relative.insert(trimmed.to_string());
                    let src_path = resolve_anchor_path(anchor_dir, trimmed);
                    if !sources.contains(&src_path) {
                        sources.push(src_path);
                    }
                }
            }
        }

        let summary_path = src_dir.join("SUMMARY.md");
        let summary = fs::read_to_string(&summary_path)
            .with_context(|| format!("Failed to read {}", summary_path.display()))?;
        for chapter_md in extract_summary_links(&summary) {
            if let Some(html_rel) = md_path_to_html(&chapter_md) {
                relative.insert(html_rel);
            }
        }

        for file in files_under(&src_dir)? {
            if file.extension().and_then(|e| e.to_str()) != Some("md") {
                relative.insert(relative_slash_path(&src_dir, &file));
            }
            if !sources.contains(&file) {
                sources.push(file);
            }
        }

        let outputs = relative
            .iter()
            .map(|rel| PlannedOutput {
                path: output_dir.join(rel),
                sources: sources.clone(),
            })
            .collect();

        Ok(BookPlan {
            output_dir,
            outputs,
            relative,
        })
    }

    /// Run `mdbook build` into a staging directory, check against the plan,
    /// and move the planned files into `output_dir`.
    fn execute_mdbook(
        &self,
        ctx: &crate::build_context::BuildContext,
        book_toml: &Path,
        instance_name: &str,
    ) -> Result<()> {
        let plan = self.plan_book(book_toml)?;
        let flat = book_toml.to_string_lossy().replace('/', "_");
        let staging = fresh_state_dir(instance_name, &format!(".{flat}.staging"))?;
        let cwd =
            std::env::current_dir().context("Failed to get current directory for mdbook build")?;
        let staging_abs = cwd.join(&staging);

        let mut cmd = Command::new(&self.config.standard.command);
        cmd.arg("build");
        cmd.arg("-d").arg(&staging_abs);
        cmd.arg(".");
        cmd.env("MDBOOK_OUTPUT__HTML__HASH_FILES", "false");
        for arg in &self.config.standard.args {
            cmd.arg(arg);
        }
        let output = run_in_anchor_dir(ctx, &mut cmd, book_toml)?;
        check_command_output(
            &output,
            format_args!("mdbook build in {}", anchor_display_dir(book_toml)),
        )?;

        install_staged(
            &staging,
            &plan.relative,
            &plan.output_dir,
            instance_name,
            &self.config.standard.command,
            self.config.loose_manifest,
        )
    }
}

/// Extract the target paths from `[title](path)` links in `SUMMARY.md`.
fn extract_summary_links(summary: &str) -> Vec<String> {
    let mut links = Vec::new();
    let bytes = summary.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'[' {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j] != b']' && bytes[j] != b'\n' {
                j += 1;
            }
            if j + 1 < bytes.len() && bytes[j] == b']' && bytes[j + 1] == b'(' {
                let start = j + 2;
                let mut end = start;
                while end < bytes.len() && bytes[end] != b')' && bytes[end] != b'\n' {
                    end += 1;
                }
                if end < bytes.len() && bytes[end] == b')' {
                    let target = summary[start..end].trim();
                    if !target.is_empty() {
                        links.push(target.to_string());
                    }
                    i = end + 1;
                    continue;
                }
            }
        }
        i += 1;
    }
    links
}

/// Convert a `SUMMARY.md` link path into the HTML path `mdbook` writes.
fn md_path_to_html(md_path: &str) -> Option<String> {
    let clean = md_path
        .strip_prefix("./")
        .or_else(|| md_path.strip_prefix('/'))
        .unwrap_or(md_path);
    if clean == "README.md" {
        Some("index.html".to_string())
    } else if let Some(prefix) = clean.strip_suffix("/README.md") {
        Some(format!("{prefix}/index.html"))
    } else {
        clean.strip_suffix(".md").map(|stem| format!("{stem}.html"))
    }
}

impl Processor for MdbookProcessor {
    fn scan_config(&self) -> &crate::config::StandardConfig {
        &self.config.standard
    }

    fn config_json(&self) -> Option<String> {
        crate::processor::ProcessorBase::config_json(&self.config)
    }

    fn clean(&self, product: &crate::graph::Product, verbose: bool) -> anyhow::Result<usize> {
        crate::processor::ProcessorBase::clean(product, &product.processor, verbose)
    }

    fn replaces_own_outputs(&self) -> bool {
        true
    }

    fn required_tools(&self) -> Vec<String> {
        vec![self.config.standard.command.clone()]
    }

    fn discover(
        &self,
        graph: &mut BuildGraph,
        file_index: &FileIndex,
        instance_name: &str,
    ) -> Result<()> {
        let Some(files) = crate::processor::scan_or_skip(&self.config.standard, file_index) else {
            return Ok(());
        };
        let config_hash = output_config_hash(
            &self.config,
            &crate::config::checksum_fields_of(instance_name),
        );
        let extra = resolve_extra_inputs(&self.config.standard.dep_inputs)?;

        let mut all_planned = Vec::new();
        for book_toml in files {
            let plan = self.plan_book(&book_toml)?;
            all_planned.extend(plan.outputs);
        }
        add_planned_products(graph, &all_planned, instance_name, &config_hash, &extra)
    }

    fn execute(&self, ctx: &crate::build_context::BuildContext, product: &Product) -> Result<()> {
        let book_toml = product.primary_input();
        let instance_name = &product.processor;
        let guard = self.run_guard(book_toml);
        guard.run(&self.config.standard.command, || {
            self.execute_mdbook(ctx, book_toml, instance_name)
        })?;
        require_planned_output(product, &self.config.standard.command)
    }
}

fn plugin_create(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| Box::new(MdbookProcessor::new(cfg)))
}
inventory::submit! {
    crate::registries::ProcessorPlugin {
        version: 2,
        name: "mdbook",
        processor_type: crate::processor::ProcessorType::MassGenerator,
        create: plugin_create,
        fields: &[
            crate::config::FieldSpec { name: "loose_manifest", ty: crate::config::FieldType::Bool,
                affects_output: false, required: false,
                doc: "Report plan-vs-build mismatches as warnings instead of failing the build" },
        ],
        omit_standard_fields: &["formats", "dep_auto"],
        scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &["book.toml"], src_exclude_dirs: &[] }),
        defaults: Some(crate::config::ProcessorDefaults { command: "mdbook", output_dir: "book", ..crate::config::ProcessorDefaults::EMPTY }),
        defconfig_json: crate::registries::default_config_json::<MdbookConfig>,
        keywords: &["markdown", "md", "rust", "documentation", "book", "html"],
        description: "Build mdbook documentation",
        is_builtin: false,
        is_rust: true,
        supports_batch: false,
        caches_outputs: true,
        max_jobs_cap: Some(1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_links_map_chapters_and_readmes_to_html() {
        let summary = "# Summary\n\n[Intro](./README.md)\n- [Ch 1](ch1.md)\n  - [Sub](./sub/README.md)\n- [Draft]()\n";
        let links = extract_summary_links(summary);
        let htmls: Vec<String> = links.iter().filter_map(|l| md_path_to_html(l)).collect();
        assert_eq!(htmls, vec!["index.html", "ch1.html", "sub/index.html"]);
    }
}
