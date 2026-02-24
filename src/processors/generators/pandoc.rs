use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::{PandocConfig, config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProductDiscovery, ProcessorType, clean_outputs, scan_root_valid, run_command, check_command_output};

pub struct PandocProcessor {
    config: PandocConfig,
}

impl PandocProcessor {
    pub fn new(config: PandocConfig) -> Self {
        Self { config }
    }

    fn should_process(&self) -> bool {
        scan_root_valid(&self.config.scan)
    }

    fn output_path(&self, source: &Path, format: &str) -> PathBuf {
        let stem = source.file_stem().unwrap_or_default();
        let parent = source.parent().unwrap_or(Path::new(""));
        let output_name = format!("{}.{}", stem.to_string_lossy(), format);
        Path::new(&self.config.output_dir).join(format).join(parent).join(output_name)
    }
}

impl ProductDiscovery for PandocProcessor {
    fn description(&self) -> &str {
        "Convert documents using pandoc"
    }

    fn processor_type(&self) -> ProcessorType {
        ProcessorType::Generator
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        self.should_process() && !file_index.scan(&self.config.scan, true).is_empty()
    }

    fn required_tools(&self) -> Vec<String> {
        vec![self.config.pandoc.clone()]
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex) -> Result<()> {
        if !self.should_process() {
            return Ok(());
        }

        let files = file_index.scan(&self.config.scan, true);
        if files.is_empty() {
            return Ok(());
        }

        let hash = Some(config_hash(&self.config));
        let extra = resolve_extra_inputs(&self.config.extra_inputs)?;

        for source in &files {
            for format in &self.config.formats {
                let output = self.output_path(source, format);

                let mut inputs = Vec::with_capacity(1 + extra.len());
                inputs.push(source.clone());
                inputs.extend_from_slice(&extra);

                graph.add_product(inputs, vec![output], crate::processors::names::PANDOC, hash.clone())?;
            }
        }

        Ok(())
    }

    fn execute(&self, product: &Product) -> Result<()> {
        let input = product.primary_input();
        let output = product.outputs.first()
            .context("pandoc product has no output")?;

        let format = output.extension()
            .context("pandoc output has no extension")?
            .to_string_lossy();

        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create pandoc output directory: {}", parent.display()))?;
        }

        let mut cmd = Command::new(&self.config.pandoc);
        cmd.arg("--from").arg(&self.config.from);
        cmd.arg("--to").arg(format.as_ref());
        for arg in &self.config.args {
            cmd.arg(arg);
        }
        cmd.arg(input);
        cmd.arg("-o").arg(output);

        let out = run_command(&mut cmd)?;
        check_command_output(&out, format_args!("pandoc {}", input.display()))
    }

    fn clean(&self, product: &Product, verbose: bool) -> Result<usize> {
        clean_outputs(product, crate::processors::names::PANDOC, verbose)
    }

    fn config_json(&self) -> Option<String> {
        serde_json::to_string(&self.config).ok()
    }
}
