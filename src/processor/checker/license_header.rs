use anyhow::Result;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::config::StandardConfig;
use crate::graph::Product;

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct LicenseHeaderConfig {
    #[serde(default)]
    pub header_lines: Vec<String>,
    #[serde(flatten)]
    pub standard: StandardConfig,
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
            if let Some(problem) = header_problem(&content, &self.config.header_lines) {
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

/// Check that `content` starts with `header`, line for line, after an
/// optional shebang line. The header may itself be the SPDX line
/// (`header_lines = ["// SPDX-License-Identifier: GPL-2.0"]`, the line the
/// Linux kernel requires first in every source file). When the header is a
/// license block instead, it may follow an SPDX line, so kernel sources
/// carry both. Returns `"<line>: <what is wrong>"` for the first mismatch
/// at the top of the file, or None when the header is there.
fn header_problem(content: &str, header: &[String]) -> Option<String> {
    let lines: Vec<&str> = content.lines().collect();
    let start = usize::from(lines.first().is_some_and(|l| l.starts_with("#!")));
    let problem = header_problem_at(&lines, start, header)?;
    let is_spdx = |l: &str| l.contains("SPDX-License-Identifier:");
    // A file opening with an SPDX line, checked against a header that is not
    // one: the header belongs after it, and that is where a mismatch is.
    let header_is_spdx = header.first().is_some_and(|h| is_spdx(h));
    if !header_is_spdx && lines.get(start).is_some_and(|l| is_spdx(l)) {
        return header_problem_at(&lines, start + 1, header);
    }
    Some(problem)
}

/// Compare `header` with `lines` from index `start` on.
fn header_problem_at(lines: &[&str], start: usize, header: &[String]) -> Option<String> {
    for (i, expected) in header.iter().enumerate() {
        let line_no = start + i + 1;
        match lines.get(start + i) {
            Some(found) if found == expected => {}
            Some(found) => {
                return Some(format!(
                    "{line_no}: license header line {} differs: expected {expected:?}, found {found:?}",
                    i + 1,
                ));
            }
            None => {
                return Some(format!(
                    "{line_no}: file ends before license header line {}: expected {expected:?}",
                    i + 1,
                ));
            }
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
                doc: "The license header every file must start with, one entry per line (after an optional shebang and SPDX line)" },
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
