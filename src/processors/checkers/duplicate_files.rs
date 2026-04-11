use anyhow::Result;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::Path;

use crate::config::DuplicateFilesConfig;
use crate::graph::Product;

pub struct DuplicateFilesProcessor {
    config: DuplicateFilesConfig,
}

impl DuplicateFilesProcessor {
    pub fn new(config: DuplicateFilesConfig) -> Self {
        Self { config }
    }

    fn execute_product(&self, _product: &Product) -> Result<()> {
        // Individual file checking is a no-op; duplicates are only detected in batch mode
        Ok(())
    }

    fn check_files(&self, files: &[&Path]) -> Result<()> {
        let mut checksums: HashMap<Vec<u8>, Vec<&Path>> = HashMap::new();

        for &file in files {
            let bytes = ctx!(std::fs::read(file), format!("Failed to read {}", file.display()))?;
            let hash = Sha256::digest(&bytes).to_vec();
            checksums.entry(hash).or_default().push(file);
        }

        let mut duplicates: Vec<String> = Vec::new();
        for paths in checksums.values() {
            if paths.len() > 1 {
                let names: Vec<String> = paths.iter().map(|p| p.display().to_string()).collect();
                duplicates.push(format!("  {}", names.join(", ")));
            }
        }

        if duplicates.is_empty() {
            Ok(())
        } else {
            duplicates.sort();
            anyhow::bail!(
                "{} set(s) of duplicate files found:\n{}",
                duplicates.len(),
                duplicates.join("\n"),
            )
        }
    }
}

impl crate::processors::Processor for DuplicateFilesProcessor {
    fn scan_config(&self) -> &crate::config::ScanConfig {
        &self.config.standard.scan
    }

    fn standard_config(&self) -> Option<&crate::config::StandardConfig> {
        Some(&self.config.standard)
    }

    fn description(&self) -> &str {
        "Detect duplicate files by content (SHA-256)"
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
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(DuplicateFilesProcessor::new(cfg)))
}
inventory::submit! {
    crate::registry::ProcessorPlugin {
        name: "duplicate_files",
        processor_type: crate::processors::ProcessorType::Checker,
        create: plugin_create,
        defconfig_json: crate::registry::default_config_json::<crate::config::DuplicateFilesConfig>,
        known_fields: crate::registry::typed_known_fields::<crate::config::DuplicateFilesConfig>,
        output_fields: crate::registry::typed_output_fields::<crate::config::DuplicateFilesConfig>,
        must_fields: crate::registry::typed_must_fields::<crate::config::DuplicateFilesConfig>,
        field_descriptions: crate::registry::typed_field_descriptions::<crate::config::DuplicateFilesConfig>,
    }
}
