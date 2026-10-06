use anyhow::{Context, Result};
use std::path::Path;

use crate::config::IyamllintConfig;
use crate::graph::Product;

pub struct IyamllintProcessor {
    config: IyamllintConfig,
}

impl IyamllintProcessor {
    pub const fn new(config: IyamllintConfig) -> Self {
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
            if let Err(e) = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&contents) {
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

impl crate::processor::Processor for IyamllintProcessor {
    fn scan_config(&self) -> &crate::config::StandardConfig {
        &self.config.standard
    }

    /// One product per scanned file (the default `discover`).
    fn discovery(&self) -> crate::processor::Discovery {
        crate::processor::Discovery::PerFile
    }

    fn required_tools(&self) -> Vec<String> {
        Vec::new()
    }

    fn execute(&self, _ctx: &crate::build_context::BuildContext, product: &Product) -> Result<()> {
        self.execute_product(product)
    }

    fn execute_batch(
        &self,
        _ctx: &crate::build_context::BuildContext,
        products: &[&Product],
    ) -> Vec<Result<()>> {
        crate::processor::execute_checker_batch_per_file(products, |file| self.check_files(&[file]))
    }
}

fn plugin_create(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| Box::new(IyamllintProcessor::new(cfg)))
}
inventory::submit! {
    crate::registries::ProcessorPlugin {
        version: 1,
        name: "iyamllint",
        processor_type: crate::processor::ProcessorType::Checker,
        create: plugin_create,
        defconfig_json: crate::registries::default_config_json::<crate::config::IyamllintConfig>,
        fields: &[],
        omit_standard_fields: &[],
        scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &[".yml", ".yaml"], src_exclude_dirs: &[] }),
        defaults: None,
        keywords: &["yaml", "yml", "linter", "validator"],
        description: "Validate YAML files (in-process)",
        is_native: true,
        is_rust: true,
        can_fix: false,
        supports_batch: true,
        max_jobs_cap: None,
    }
}
