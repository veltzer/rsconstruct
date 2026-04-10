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
    fn description(&self) -> &str {
        "Check files for non-ASCII characters"
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