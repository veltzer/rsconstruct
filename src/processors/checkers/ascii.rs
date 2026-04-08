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

impl_checker!(AsciiProcessor,
    config: config,
    description: "Check files for non-ASCII characters",
    name: crate::processors::names::ASCII,
    execute: execute_product,
    config_json: true,
    native: true,
    batch: check_files,
);
