use anyhow::Result;
use std::path::Path;

use crate::config::EncodingConfig;
use crate::graph::Product;

pub struct EncodingProcessor {
    config: EncodingConfig,
}

impl EncodingProcessor {
    pub fn new(config: EncodingConfig) -> Self {
        Self { config }
    }

    fn execute_product(&self, product: &Product) -> Result<()> {
        self.check_files(&[product.primary_input()])
    }

    fn check_files(&self, files: &[&Path]) -> Result<()> {
        let mut errors = Vec::new();

        for file in files {
            let bytes = std::fs::read(file)?;
            if let Err(msg) = validate_utf8(&bytes) {
                errors.push(format!("{}: {}", file.display(), msg));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            anyhow::bail!("Encoding errors found:\n{}", errors.join("\n"))
        }
    }
}

/// Validate that bytes are valid UTF-8 and optionally check for a BOM.
fn validate_utf8(bytes: &[u8]) -> std::result::Result<(), String> {
    // Check for UTF-8 BOM (some tools dislike it)
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return Err("file has UTF-8 BOM (byte order mark)".into());
    }
    // Check for UTF-16 BOMs (wrong encoding)
    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        return Err("file appears to be UTF-16 encoded".into());
    }
    // Validate UTF-8
    if let Err(e) = std::str::from_utf8(bytes) {
        let byte_pos = e.valid_up_to();
        // Find line number
        let line_num = bytes[..byte_pos].iter().filter(|&&b| b == b'\n').count() + 1;
        return Err(format!("invalid UTF-8 at byte {} (line {})", byte_pos, line_num));
    }
    Ok(())
}

impl_checker!(EncodingProcessor,
    config: config,
    description: "Validate that text files are valid UTF-8 without BOM",
    name: crate::processors::names::ENCODING,
    execute: execute_product,
    config_json: true,
    native: true,
    batch: check_files,
);
