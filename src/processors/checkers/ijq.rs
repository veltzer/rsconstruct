use anyhow::{Context, Result};
use std::path::Path;

use crate::config::IjqConfig;
use crate::graph::Product;

pub struct IjqProcessor {
    config: IjqConfig,
}

impl IjqProcessor {
    pub fn new(config: IjqConfig) -> Self {
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

impl crate::processors::ProductDiscovery for IjqProcessor {
    fn description(&self) -> &str {
        "Validate JSON files (in-process)"
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

    fn execute(&self, product: &crate::graph::Product) -> anyhow::Result<()> {
        self.execute_product(product)
    }

    fn config_json(&self) -> Option<String> {
        serde_json::to_string(&self.config).ok()
    }

    fn is_native(&self) -> bool { true }

    fn supports_batch(&self) -> bool { self.config.batch }

    fn execute_batch(&self, products: &[&crate::graph::Product]) -> Vec<anyhow::Result<()>> {
        crate::processors::execute_checker_batch(products, |files| self.check_files(files))
    }

    fn max_jobs(&self) -> Option<usize> {
        self.config.max_jobs
    }
}