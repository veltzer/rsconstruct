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

impl_checker!(IjsonlintProcessor,
    config: config,
    description: "Lint JSON files (in-process)",
    name: crate::processors::names::IJSONLINT,
    execute: execute_product,
    config_json: true,
    batch: check_files,
);
