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

impl crate::processors::ProductDiscovery for IyamllintProcessor {
    fn scan_config(&self) -> &crate::config::ScanConfig {
        &self.config.scan
    }

    fn standard_config(&self) -> Option<&crate::config::StandardConfig> {
        Some(&self.config)
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




    fn supports_batch(&self) -> bool { self.config.batch }

    fn execute_batch(&self, products: &[&Product]) -> Vec<Result<()>> {
        crate::processors::execute_checker_batch(products, |files| self.check_files(files))
    }
}

inventory::submit! {
    &crate::registry::typed_plugin::<crate::config::IyamllintConfig>(
        "iyamllint", |cfg| Box::new(IyamllintProcessor::new(cfg))
    ) as &dyn crate::registry::RegistryOps
}
