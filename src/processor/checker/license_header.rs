use anyhow::Result;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::config::StandardConfig;
use crate::graph::Product;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LicenseHeaderConfig {
    #[serde(default)]
    pub header_lines: Vec<String>,
    /// Lines that may come right before the header, all of them or none.
    #[serde(default)]
    pub optional_prefix_lines: Vec<String>,
    /// Whether a leading `#!` line is skipped before looking for the header.
    #[serde(default = "default_skip_shebang")]
    pub skip_shebang: bool,
    #[serde(flatten)]
    pub standard: StandardConfig,
}

const fn default_skip_shebang() -> bool {
    true
}

impl Default for LicenseHeaderConfig {
    fn default() -> Self {
        Self {
            header_lines: Vec::new(),
            optional_prefix_lines: Vec::new(),
            skip_shebang: default_skip_shebang(),
            standard: StandardConfig::default(),
        }
    }
}

pub struct LicenseHeaderProcessor {
    config: LicenseHeaderConfig,
}

impl LicenseHeaderProcessor {
    pub const fn new(config: LicenseHeaderConfig) -> Self {
        Self { config }
    }

    fn execute_product(&self, product: &Product) -> Result<()> {
        self.check_files(&[product.primary_input()])
    }

    fn check_files(&self, files: &[&Path]) -> Result<()> {
        let mut errors = Vec::new();

        for &file in files {
            let content = crate::errors::ctx(
                std::fs::read_to_string(file),
                &format!("Failed to read {}", file.display()),
            )?;
            if let Some(problem) = header_problem(&content, &self.config) {
                errors.push(format!("{}:{}", file.display(), problem));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            anyhow::bail!(
                "{} file(s) missing license headers:\n{}",
                errors.len(),
                errors.join("\n"),
            )
        }
    }
}

/// Check that `content` starts with the header text: `header_lines`, each
/// terminated by a newline, after an optional `#!` line (`skip_shebang`)
/// and optionally preceded by `optional_prefix_lines`. Line endings are
/// read as Python's text mode reads them (`\r\n` and `\r` become `\n`).
/// Returns `"<line>: <what is wrong>"` for the first mismatch, or None when
/// the header is there.
fn header_problem(content: &str, config: &LicenseHeaderConfig) -> Option<String> {
    let text = content.replace("\r\n", "\n").replace('\r', "\n");
    let (mut body, mut skipped) = (text.as_str(), 0);
    if config.skip_shebang && body.starts_with("#!") {
        body = body.split_once('\n').map_or("", |(_, rest)| rest);
        skipped = 1;
    }
    let problem = header_mismatch(body, &config.header_lines, skipped)?;
    if !config.optional_prefix_lines.is_empty()
        && let Some(rest) = body.strip_prefix(&as_text(&config.optional_prefix_lines))
    {
        // The prefix is there, so the header belongs after it, and that is
        // where a mismatch is reported.
        return header_mismatch(
            rest,
            &config.header_lines,
            skipped + config.optional_prefix_lines.len(),
        );
    }
    Some(problem)
}

/// `lines` as text, each line terminated by a newline.
fn as_text(lines: &[String]) -> String {
    let mut text = String::new();
    for line in lines {
        text.push_str(line);
        text.push('\n');
    }
    text
}

/// Describe where `text` stops starting with `header` (each line followed
/// by a newline), or None when it does. `skipped` is how many file lines
/// precede `text`, for the reported line number.
fn header_mismatch(text: &str, header: &[String], skipped: usize) -> Option<String> {
    let mut pieces = text.split_inclusive('\n');
    for (i, expected) in header.iter().enumerate() {
        let line_no = skipped + i + 1;
        let n = i + 1;
        let Some(piece) = pieces.next() else {
            return Some(format!(
                "{line_no}: file ends before license header line {n}: expected {expected:?}"
            ));
        };
        let found = piece.strip_suffix('\n').unwrap_or(piece);
        if found != expected {
            return Some(format!(
                "{line_no}: license header line {n} differs: expected {expected:?}, found {found:?}"
            ));
        }
        if !piece.ends_with('\n') {
            return Some(format!(
                "{line_no}: file ends without a newline after license header line {n}"
            ));
        }
    }
    None
}

impl crate::processor::Processor for LicenseHeaderProcessor {
    fn scan_config(&self) -> &crate::config::StandardConfig {
        &self.config.standard
    }

    // Serialize the FULL config (the trait default covers StandardConfig
    // only), so the extra fields reach config-change detection.
    fn config_json(&self) -> Option<String> {
        crate::processor::ProcessorBase::config_json(&self.config)
    }

    fn auto_detect(&self, file_index: &crate::file_index::FileIndex) -> bool {
        crate::processor::checker_auto_detect(&self.config.standard, file_index)
    }

    fn required_tools(&self) -> Vec<String> {
        Vec::new()
    }

    fn discover(
        &self,
        graph: &mut crate::graph::BuildGraph,
        file_index: &crate::file_index::FileIndex,
        instance_name: &str,
    ) -> anyhow::Result<()> {
        crate::processor::discover_checker_products(
            graph,
            &self.config.standard,
            file_index,
            &self.config.standard.dep_inputs,
            &self.config.standard.dep_auto,
            &self.config,
            &crate::config::checksum_fields_of(instance_name),
            instance_name,
        )
    }

    fn execute(&self, _ctx: &crate::build_context::BuildContext, product: &Product) -> Result<()> {
        self.execute_product(product)
    }

    fn execute_batch(
        &self,
        _ctx: &crate::build_context::BuildContext,
        products: &[&Product],
    ) -> Vec<Result<()>> {
        crate::processor::execute_checker_batch_per_file(products, |file| self.check_files(&[file]))
    }
}

fn plugin_create(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| {
        Box::new(LicenseHeaderProcessor::new(cfg))
    })
}
inventory::submit! {
    crate::registries::ProcessorPlugin {
        version: 1,
        name: "license_header",
        processor_type: crate::processor::ProcessorType::Checker,
        create: plugin_create,
        fields: &[
            crate::config::FieldSpec { name: "header_lines", ty: crate::config::FieldType::StringArray,
                affects_output: true, required: true,
                doc: "The license header every file must start with, one entry per line" },
            crate::config::FieldSpec { name: "optional_prefix_lines", ty: crate::config::FieldType::StringArray,
                affects_output: true, required: false,
                doc: "Lines that may come right before the header, all or none (e.g. a kernel SPDX line)" },
            crate::config::FieldSpec { name: "skip_shebang", ty: crate::config::FieldType::Bool,
                affects_output: true, required: false,
                doc: "Skip a leading #! line before looking for the header" },
        ],
        omit_standard_fields: &["command", "formats", "output_dir"],
        scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &[".py", ".rs", ".js", ".ts", ".c", ".cc", ".h", ".hh", ".java", ".rb", ".go", ".sh", ".bash"], src_exclude_dirs: &[] }),
        defaults: None,
        defconfig_json: crate::registries::default_config_json::<LicenseHeaderConfig>,
        keywords: &["checker", "license", "header", "copyright"],
        description: "Verify source files contain required license headers",
        is_native: true,
        is_rust: true,
        can_fix: false,
        supports_batch: true,
        max_jobs_cap: None,
    }
}
