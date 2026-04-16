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

impl crate::processors::Processor for IjsonlintProcessor {
    fn scan_config(&self) -> &crate::config::StandardConfig {
        &self.config.standard
    }

    fn standard_config(&self) -> Option<&crate::config::StandardConfig> {
        Some(&self.config.standard)
    }

    fn description(&self) -> &str {
        "Lint JSON files (in-process)"
    }


    fn required_tools(&self) -> Vec<String> {
        Vec::new()
    }


    fn execute(&self, ctx: &crate::build_context::BuildContext, product: &Product) -> Result<()> {
        self.execute_product(product)
    }


    fn is_native(&self) -> bool { true }




    fn supports_batch(&self) -> bool { self.config.standard.batch }

    fn execute_batch(&self, ctx: &crate::build_context::BuildContext, products: &[&Product]) -> Vec<Result<()>> {
        crate::processors::execute_checker_batch(ctx, products, |ctx, files| self.check_files(files))
    }
}

fn plugin_create(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| Box::new(IjsonlintProcessor::new(cfg)))
}
inventory::submit! {
    crate::registries::ProcessorPlugin {
        version: 1,
        name: "ijsonlint",
        processor_type: crate::processors::ProcessorType::Checker,
        create: plugin_create,
        defconfig_json: crate::registries::default_config_json::<crate::config::IjsonlintConfig>,
        known_fields: crate::registries::typed_known_fields::<crate::config::IjsonlintConfig>,
        output_fields: crate::registries::typed_output_fields::<crate::config::IjsonlintConfig>,
        must_fields: crate::registries::typed_must_fields::<crate::config::IjsonlintConfig>,
        field_descriptions: crate::registries::typed_field_descriptions::<crate::config::IjsonlintConfig>,
    }
}
