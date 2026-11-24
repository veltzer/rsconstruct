use anyhow::Result;
use std::path::Path;

use crate::config::CheckerConfig;
use crate::graph::Product;

pub struct EncodingProcessor {
    config: CheckerConfig,
}

impl EncodingProcessor {
    pub const fn new(config: CheckerConfig) -> Self {
        Self { config }
    }

    fn execute_product(product: &Product) -> Result<()> {
        Self::check_files(&[product.primary_input()])
    }

    fn check_files(files: &[&Path]) -> Result<()> {
        let mut errors = Vec::new();

        for file in files {
            let bytes = crate::errors::ctx(
                std::fs::read(file),
                &format!("Failed to read {}", file.display()),
            )?;
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
        // Find line number.
        let line_num = bytecount::count(&bytes[..byte_pos], b'\n') + 1;
        return Err(format!(
            "invalid UTF-8 at byte {byte_pos} (line {line_num})"
        ));
    }
    Ok(())
}

impl crate::processor::Processor for EncodingProcessor {
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
        Self::execute_product(product)
    }

    fn execute_batch(
        &self,
        _ctx: &crate::build_context::BuildContext,
        products: &[&Product],
    ) -> Vec<Result<()>> {
        crate::processor::execute_checker_batch_per_file(products, |file| {
            Self::check_files(&[file])
        })
    }
}

fn plugin_create(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| Box::new(EncodingProcessor::new(cfg)))
}
inventory::submit! {
    crate::registries::ProcessorPlugin {
        version: 1,
        name: "encoding",
        processor_type: crate::processor::ProcessorType::Checker,
        create: plugin_create,
        defconfig_json: crate::registries::default_config_json::<crate::config::CheckerConfig>,
        fields: &[],
        omit_standard_fields: &["command", "formats", "args", "output_dir", "required_tools"],
        scan_defaults: Some(crate::config::ScanDefaultsData { src_extensions: &[".py", ".rs", ".js", ".ts", ".c", ".cc", ".h", ".hh", ".java", ".rb", ".go", ".sh", ".bash", ".lua", ".pl", ".pm", ".php", ".md", ".yaml", ".yml", ".json", ".toml", ".xml", ".html", ".htm", ".css", ".scss", ".sass", ".tex", ".txt"], src_exclude_dirs: &[] }),
        defaults: None,
        keywords: &["checker", "encoding", "utf8", "text", "validator"],
        description: "Validate that text files are valid UTF-8 without BOM",
        implementation: crate::registries::Implementation::Builtin,
        supports_batch: true,
        caches_outputs: true,
        sandboxable: true,
        max_jobs_cap: None,
    }
}
