use anyhow::{Context, Result};
use std::path::Path;

use crate::config::ItaploConfig;
use crate::graph::Product;

pub struct ItaploProcessor {
    config: ItaploConfig,
}

impl ItaploProcessor {
    pub fn new(config: ItaploConfig) -> Self {
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
            if let Err(e) = toml::from_str::<toml::Value>(&contents) {
                errors.push(format!("{}: {}", file.display(), e));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            anyhow::bail!("Invalid TOML:\n{}", errors.join("\n"))
        }
    }
}

impl_checker!(ItaploProcessor,
    config: config,
    description: "Validate TOML files (in-process)",
    name: crate::processors::names::ITAPLO,
    execute: execute_product,
    config_json: true,
    native: true,
    batch: check_files,
);
