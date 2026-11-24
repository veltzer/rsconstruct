use anyhow::Result;
use regex::Regex;
use std::path::Path;
use std::sync::LazyLock;

use crate::config::CheckerConfig;
use crate::graph::Product;

pub struct MarpImagesProcessor {
    config: CheckerConfig,
}

static IMAGE_RE: LazyLock<Regex> = LazyLock::new(|| {
    // The path capture must stop before whitespace, or `![alt](img.png "Title")`
    // captures "img.png " (trailing space) and the file check always fails.
    Regex::new(r#"!\[[^\]]*\]\(\s*([^)"\s]+)(?:\s+"[^"]*")?\s*\)"#).unwrap()
});

impl MarpImagesProcessor {
    pub const fn new(config: CheckerConfig) -> Self {
        Self { config }
    }

    fn execute_product(product: &Product) -> Result<()> {
        Self::check_files(&[product.primary_input()])
    }

    /// The local image references of a markdown text, as (1-based line,
    /// path as written). External URLs and data URIs are not files.
    fn image_refs(content: &str) -> Vec<(usize, String)> {
        let mut refs = Vec::new();
        for (line_num, line) in content.lines().enumerate() {
            for cap in IMAGE_RE.captures_iter(line) {
                let image_path = &cap[1];
                if image_path.starts_with("http://")
                    || image_path.starts_with("https://")
                    || image_path.starts_with("data:")
                {
                    continue;
                }
                refs.push((line_num + 1, image_path.to_string()));
            }
        }
        refs
    }

    fn check_files(files: &[&Path]) -> Result<()> {
        let mut errors = Vec::new();

        for &file in files {
            let content = crate::errors::ctx(
                std::fs::read_to_string(file),
                &format!("Failed to read {}", file.display()),
            )?;
            let dir = crate::processor::parent_dir(file);

            for (line, image_path) in Self::image_refs(&content) {
                let resolved = dir.join(&image_path);
                if !resolved.exists() {
                    errors.push(format!(
                        "{}:{}: missing image: {}",
                        file.display(),
                        line,
                        image_path,
                    ));
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

impl crate::processor::Processor for MarpImagesProcessor {
    fn scan_config(&self) -> &crate::config::StandardConfig {
        &self.config.standard
    }

    /// One product per scanned file.
    fn discovery(&self) -> crate::processor::Discovery {
        crate::processor::Discovery::PerFile
    }

    /// One product per markdown file, whose inputs are the file, the
    /// configured dep files, and every image it references that exists.
    ///
    /// The verdict depends on those images, so they must be inputs: with
    /// the markdown file as the only one, deleting an image left the cached
    /// pass in place. A referenced image that is missing is not an input;
    /// the check then fails, and a failure is never cached.
    fn discover(
        &self,
        graph: &mut crate::graph::BuildGraph,
        file_index: &crate::file_index::FileIndex,
        instance_name: &str,
    ) -> Result<()> {
        let files = file_index.scan(&self.config.standard, true);
        if files.is_empty() {
            return Ok(());
        }
        let hash = Some(crate::config::output_config_hash(
            &self.config,
            &crate::config::checksum_fields_of(instance_name),
        ));
        let extra = crate::processor::resolve_dep_inputs(
            &self.config.standard.dep_inputs,
            &self.config.standard.dep_auto,
        )?;
        for file in files {
            let mut inputs = vec![file.clone()];
            inputs.extend_from_slice(&extra);
            // A markdown file another product generates is not on disk
            // yet; its images are found when it is.
            if file.is_file() {
                let content = crate::errors::ctx(
                    std::fs::read_to_string(&file),
                    &format!("Failed to read {}", file.display()),
                )?;
                let dir = crate::processor::parent_dir(&file);
                for (_, image_path) in Self::image_refs(&content) {
                    let image = crate::analyzers::lexical_normalize(&dir.join(&image_path));
                    if (image.is_file() || file_index.contains(&image)) && !inputs.contains(&image)
                    {
                        inputs.push(image);
                    }
                }
            }
            graph.add_product(inputs, vec![], instance_name, hash.clone())?;
        }
        Ok(())
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
    crate::registries::deserialize_and_create(toml, |cfg| Box::new(MarpImagesProcessor::new(cfg)))
}
inventory::submit! {
    crate::registries::ProcessorPlugin {
        version: 1,
        name: "marp_images",
        processor_type: crate::processor::ProcessorType::Checker,
        create: plugin_create,
        fields: &[],
        omit_standard_fields: &["command", "formats", "args", "output_dir", "required_tools"],
        scan_defaults: Some(crate::config::ScanDefaultsData { src_extensions: &[".md"], src_exclude_dirs: &[] }),
        defaults: None,
        defconfig_json: crate::registries::default_config_json::<crate::config::CheckerConfig>,
        keywords: &["markdown", "marp", "images", "checker", "presentation"],
        description: "Validate image references in Marp markdown presentations",
        implementation: crate::registries::Implementation::Builtin,
        supports_batch: true,
        caches_outputs: true,
        sandboxable: true,
        max_jobs_cap: None,
    }
}
