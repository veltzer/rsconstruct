use anyhow::Result;
use std::path::Path;

use crate::config::AsciiConfig;
use crate::graph::Product;

pub struct AsciiProcessor {
    config: AsciiConfig,
}

impl AsciiProcessor {
    pub fn new(config: AsciiConfig) -> Self {
        Self { config }
    }

    fn execute_product(&self, product: &Product) -> Result<()> {
        self.check_files(&[product.primary_input()])
    }

    fn check_files(&self, files: &[&Path]) -> Result<()> {
        let mut errors = Vec::new();

        for file in files {
            let content = ctx!(std::fs::read(file), format!("Failed to read {}", file.display()))?;
            let mut line_num = 1usize;
            let mut col = 1usize;
            let mut line_errors: Vec<String> = Vec::new();

            for &byte in &content {
                if byte == b'\n' {
                    line_num += 1;
                    col = 1;
                } else if !byte.is_ascii() {
                    line_errors.push(format!(
                        "{}:{}:{}: non-ASCII byte 0x{:02x}",
                        file.display(), line_num, col, byte,
                    ));
                    col += 1;
                } else {
                    col += 1;
                }
            }

            if !line_errors.is_empty() {
                errors.extend(line_errors);
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            anyhow::bail!("Non-ASCII characters found:\n{}", errors.join("\n"))
        }
    }
}

impl crate::processors::Processor for AsciiProcessor {
    fn scan_config(&self) -> &crate::config::ScanConfig {
        &self.config.standard.scan
    }

    fn standard_config(&self) -> Option<&crate::config::StandardConfig> {
        Some(&self.config.standard)
    }

    fn description(&self) -> &str {
        "Check files for non-ASCII characters"
    }


    fn required_tools(&self) -> Vec<String> {
        Vec::new()
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
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(AsciiProcessor::new(cfg)))
}
inventory::submit! {
    crate::registry::ProcessorPlugin {
        name: "ascii",
        processor_type: crate::processors::ProcessorType::Checker,
        create: plugin_create,
        defconfig_json: crate::registry::default_config_json::<crate::config::AsciiConfig>,
        known_fields: crate::registry::typed_known_fields::<crate::config::AsciiConfig>,
        output_fields: crate::registry::typed_output_fields::<crate::config::AsciiConfig>,
        must_fields: crate::registry::typed_must_fields::<crate::config::AsciiConfig>,
        field_descriptions: crate::registry::typed_field_descriptions::<crate::config::AsciiConfig>,
    }
}
