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
            let content = ctx!(std::fs::read_to_string(file), format!("Failed to read {}", file.display()))?;
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

impl crate::processors::Processor for LicenseHeaderProcessor {
    fn scan_config(&self) -> &crate::config::StandardConfig {
        &self.config.standard
    }

    fn standard_config(&self) -> Option<&crate::config::StandardConfig> {
        Some(&self.config.standard)
    }

    fn description(&self) -> &str {
        "Verify source files contain required license headers"
    }

    fn auto_detect(&self, file_index: &crate::file_index::FileIndex) -> bool {
        crate::processors::checker_auto_detect(&self.config.standard.scan, file_index)
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
            graph, &self.config.standard.scan, file_index,
            &self.config.standard.dep_inputs, &self.config.standard.dep_auto,
            &self.config, instance_name,
        )
    }

    fn execute(&self, product: &Product) -> Result<()> {
        self.execute_product(product)
    }

    fn is_native(&self) -> bool { true }

    fn supports_batch(&self) -> bool { self.config.standard.batch }

    fn execute_batch(&self, products: &[&Product]) -> Vec<Result<()>> {
        crate::processors::execute_checker_batch(products, |files| self.check_files(files))
    }
}

fn plugin_create(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(LicenseHeaderProcessor::new(cfg)))
}
inventory::submit! {
    crate::registry::ProcessorPlugin {
        name: "license_header",
        processor_type: crate::processors::ProcessorType::Checker,
        create: plugin_create,
        defconfig_json: crate::registry::default_config_json::<crate::config::LicenseHeaderConfig>,
        known_fields: crate::registry::typed_known_fields::<crate::config::LicenseHeaderConfig>,
        output_fields: crate::registry::typed_output_fields::<crate::config::LicenseHeaderConfig>,
        must_fields: crate::registry::typed_must_fields::<crate::config::LicenseHeaderConfig>,
        field_descriptions: crate::registry::typed_field_descriptions::<crate::config::LicenseHeaderConfig>,
    }
}
