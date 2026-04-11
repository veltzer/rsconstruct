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

impl crate::processors::ProductDiscovery for LicenseHeaderProcessor {
    fn scan_config(&self) -> &crate::config::ScanConfig {
        &self.config.scan
    }


    fn description(&self) -> &str {
        "Verify source files contain required license headers"
    }

    fn auto_detect(&self, file_index: &crate::file_index::FileIndex) -> bool {
        crate::processors::checker_auto_detect(&self.config.scan, file_index)
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
        crate::processors::checker_discover(
            graph, &self.config.scan, file_index,
            &self.config.dep_inputs, &self.config.dep_auto,
            &self.config, instance_name,
        )
    }

    fn execute(&self, product: &Product) -> Result<()> {
        self.execute_product(product)
    }

    fn config_json(&self) -> Option<String> {
        serde_json::to_string(&self.config).ok()
    }

    fn is_native(&self) -> bool { true }

    fn supports_batch(&self) -> bool { self.config.batch }

    fn execute_batch(&self, products: &[&Product]) -> Vec<Result<()>> {
        crate::processors::execute_checker_batch(products, |files| self.check_files(files))
    }

    fn max_jobs(&self) -> Option<usize> {
        self.config.max_jobs
    }
}

inventory::submit! {
    &crate::registry::typed_plugin::<crate::config::LicenseHeaderConfig>(
        "license_header", |cfg| Box::new(LicenseHeaderProcessor::new(cfg))
    ) as &dyn crate::registry::RegistryOps
}
