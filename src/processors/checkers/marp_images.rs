use anyhow::Result;
use regex::Regex;
use std::path::Path;
use std::sync::LazyLock;

use crate::config::MarpImagesConfig;
use crate::graph::Product;

pub struct MarpImagesProcessor {
    config: MarpImagesConfig,
}

static IMAGE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"!\[[^\]]*\]\(([^)"]+)(?:"[^"]*")?\)"#).unwrap()
});

impl MarpImagesProcessor {
    pub fn new(config: MarpImagesConfig) -> Self {
        Self { config }
    }

    fn execute_product(&self, product: &Product) -> Result<()> {
        self.check_files(&[product.primary_input()])
    }

    fn check_files(&self, files: &[&Path]) -> Result<()> {
        let mut errors = Vec::new();

        for &file in files {
            let content = std::fs::read_to_string(file)?;
            let dir = file.parent().unwrap_or(Path::new("."));

            for (line_num, line) in content.lines().enumerate() {
                for cap in IMAGE_RE.captures_iter(line) {
                    let image_path = &cap[1];
                    // Skip external URLs and data URIs
                    if image_path.starts_with("http://")
                        || image_path.starts_with("https://")
                        || image_path.starts_with("data:")
                    {
                        continue;
                    }
                    let resolved = dir.join(image_path);
                    if !resolved.exists() {
                        errors.push(format!(
                            "{}:{}: missing image: {}",
                            file.display(),
                            line_num + 1,
                            image_path,
                        ));
                    }
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            anyhow::bail!("Missing image references:\n{}", errors.join("\n"))
        }
    }
}

impl crate::processors::ProductDiscovery for MarpImagesProcessor {
    fn scan_config(&self) -> &crate::config::ScanConfig {
        &self.config.scan
    }

    fn standard_config(&self) -> Option<&crate::config::StandardConfig> {
        Some(&self.config)
    }

    fn description(&self) -> &str {
        "Validate image references in Marp markdown presentations"
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
    &crate::registry::typed_plugin::<crate::config::MarpImagesConfig>(
        "marp_images", |cfg| Box::new(MarpImagesProcessor::new(cfg))
    ) as &dyn crate::registry::RegistryOps
}
