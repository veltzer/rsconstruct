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

impl_checker!(IjqProcessor,
    config: config,
    description: "Validate JSON files (in-process)",
    name: crate::processors::names::IJQ,
    execute: execute_product,
    config_json: true,
    native: true,
    batch: check_files,
);
