use anyhow::Result;
use std::path::Path;

use crate::config::LicenseHeaderConfig;
use crate::graph::Product;

pub struct LicenseHeaderProcessor {
    config: LicenseHeaderConfig,
}

impl LicenseHeaderProcessor {
    pub fn new(config: LicenseHeaderConfig) -> Self {
        Self { config }
    }

    fn execute_product(&self, product: &Product) -> Result<()> {
        self.check_files(&[product.primary_input()])
    }

    fn check_files(&self, files: &[&Path]) -> Result<()> {
        if self.config.header_lines.is_empty() {
            return Ok(());
        }
        let mut errors = Vec::new();

        for &file in files {
            let content = std::fs::read_to_string(file)?;
            let mut lines = content.lines();

            // Skip shebang line if present
            let mut first_line = lines.next().unwrap_or("");
            if first_line.starts_with("#!") {
                first_line = lines.next().unwrap_or("");
            }

            let file_lines: Vec<&str> = std::iter::once(first_line).chain(lines).collect();

            let mut found = false;
            for header_line in &self.config.header_lines {
                if file_lines.iter().any(|l| l.contains(header_line.as_str())) {
                    found = true;
                    break;
                }
            }

            if !found {
                errors.push(format!(
                    "{}: missing license header (expected one of: {})",
                    file.display(),
                    self.config.header_lines.join(", "),
                ));
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

impl_checker!(LicenseHeaderProcessor,
    config: config,
    description: "Verify source files contain required license headers",
    name: crate::processors::names::LICENSE_HEADER,
    execute: execute_product,
    config_json: true,
    native: true,
    batch: check_files,
);
