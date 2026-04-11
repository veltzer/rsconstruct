use anyhow::{Context, Result};
use std::path::Path;

use crate::config::IjsonlintConfig;
use crate::graph::Product;

pub struct IjsonlintProcessor {
    config: IjsonlintConfig,
}

impl IjsonlintProcessor {
    pub fn new(config: IjsonlintConfig) -> Self {
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
            if let Err(e) = serde_json::from_str::<serde_json::Value>(&contents) {
                errors.push(format!("{}: {}", file.display(), e));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            anyhow::bail!("Invalid JSON:\n{}", errors.join("\n"))
        }
    }
}

impl crate::processors::ProductDiscovery for IjsonlintProcessor {
    fn scan_config(&self) -> &crate::config::ScanConfig {
        &self.config.scan
    }

    fn standard_config(&self) -> Option<&crate::config::StandardConfig> {
        Some(&self.config)
    }

    fn description(&self) -> &str {
        "Lint JSON files (in-process)"
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
    &crate::registry::typed_plugin::<crate::config::IjsonlintConfig>(
        "ijsonlint", |cfg| Box::new(IjsonlintProcessor::new(cfg))
    ) as &dyn crate::registry::RegistryOps
}
