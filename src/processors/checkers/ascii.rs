use anyhow::Result;
use std::path::Path;

use crate::config::AsciiConfig;
use crate::graph::Product;

pub struct AsciiProcessor {
    config: AsciiConfig,
}

impl AsciiProcessor {
    pub fn new(config: AsciiConfig) -> Self {
        Self { config }
    }

    fn execute_product(&self, product: &Product) -> Result<()> {
        self.check_files(&[product.primary_input()])
    }

    fn check_files(&self, files: &[&Path]) -> Result<()> {
        let mut errors = Vec::new();

        for file in files {
            let content = std::fs::read(file)?;
            let mut line_num = 1usize;
            let mut col = 1usize;
            let mut line_errors: Vec<String> = Vec::new();

            for &byte in &content {
                if byte == b'\n' {
                    line_num += 1;
                    col = 1;
                } else if !byte.is_ascii() {
                    line_errors.push(format!(
                        "{}:{}:{}: non-ASCII byte 0x{:02x}",
                        file.display(), line_num, col, byte,
                    ));
                    col += 1;
                } else {
                    col += 1;
                }
            }

            if !line_errors.is_empty() {
                errors.extend(line_errors);
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            anyhow::bail!("Non-ASCII characters found:\n{}", errors.join("\n"))
        }
    }
}

impl crate::processors::ProductDiscovery for AsciiProcessor {
    fn scan_config(&self) -> &crate::config::ScanConfig {
        &self.config.scan
    }

    fn standard_config(&self) -> Option<&crate::config::StandardConfig> {
        Some(&self.config)
    }

    fn description(&self) -> &str {
        "Check files for non-ASCII characters"
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
    &crate::registry::typed_plugin::<crate::config::AsciiConfig>(
        "ascii", |cfg| Box::new(AsciiProcessor::new(cfg))
    ) as &dyn crate::registry::RegistryOps
}
