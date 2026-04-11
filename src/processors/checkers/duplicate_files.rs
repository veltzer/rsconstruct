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
            let bytes = std::fs::read(file)?;
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

impl crate::processors::ProductDiscovery for DuplicateFilesProcessor {
    fn scan_config(&self) -> &crate::config::ScanConfig {
        &self.config.scan
    }

    fn standard_config(&self) -> Option<&crate::config::StandardConfig> {
        Some(&self.config)
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




    fn supports_batch(&self) -> bool { self.config.batch }

    fn execute_batch(&self, products: &[&Product]) -> Vec<Result<()>> {
        crate::processors::execute_checker_batch(products, |files| self.check_files(files))
    }
}

inventory::submit! {
    &crate::registry::typed_plugin::<crate::config::DuplicateFilesConfig>(
        "duplicate_files", |cfg| Box::new(DuplicateFilesProcessor::new(cfg))
    ) as &dyn crate::registry::RegistryOps
}
