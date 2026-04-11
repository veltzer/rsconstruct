use anyhow::{Context, Result};
use std::path::Path;

use crate::config::IyamllintConfig;
use crate::graph::Product;

pub struct IyamllintProcessor {
    config: IyamllintConfig,
}

impl IyamllintProcessor {
    pub fn new(config: IyamllintConfig) -> Self {
        Self { config }
    }

    fn execute_product(&self, product: &Product) -> Result<()> {
        self.check_files(&[product.primary_input()])
    }

    fn check_files(&self, files: &[&Path]) -> Result<()> {
        let mut errors = Vec::new();

        for file in files {
            let contents = std::fs::read_to_string(file)
                .with_context(|| format!("Failed to read {}", file.display()))?;
            if let Err(e) = serde_yaml::from_str::<serde_yaml::Value>(&contents) {
                errors.push(format!("{}: {}", file.display(), e));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            anyhow::bail!("Invalid YAML:\n{}", errors.join("\n"))
        }
    }
}

impl crate::processors::Processor for IyamllintProcessor {
    fn scan_config(&self) -> &crate::config::ScanConfig {
        &self.config.standard.scan
    }

    fn standard_config(&self) -> Option<&crate::config::StandardConfig> {
        Some(&self.config.standard)
    }

    fn description(&self) -> &str {
        "Validate YAML files (in-process)"
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
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(IyamllintProcessor::new(cfg)))
}
inventory::submit! {
    crate::registry::ProcessorPlugin {
        name: "iyamllint",
        processor_type: crate::processors::ProcessorType::Checker,
        create: plugin_create,
        defconfig_json: crate::registry::default_config_json::<crate::config::IyamllintConfig>,
        known_fields: crate::registry::typed_known_fields::<crate::config::IyamllintConfig>,
        output_fields: crate::registry::typed_output_fields::<crate::config::IyamllintConfig>,
        must_fields: crate::registry::typed_must_fields::<crate::config::IyamllintConfig>,
        field_descriptions: crate::registry::typed_field_descriptions::<crate::config::IyamllintConfig>,
    }
}
