use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::{MarkdownGenConfig, config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProductDiscovery, ProcessorType, clean_outputs, scan_root_valid, run_command_capture, check_command_output};

pub struct MarkdownProcessor {
    config: MarkdownGenConfig,
}

impl MarkdownProcessor {
    pub fn new(config: MarkdownGenConfig) -> Self {
        Self { config }
    }

    fn should_process(&self) -> bool {
        scan_root_valid(&self.config.scan)
    }

    fn output_path(&self, source: &Path) -> PathBuf {
        let stem = source.file_stem().unwrap_or_default();
        let parent = source.parent().unwrap_or(Path::new(""));
        let output_name = format!("{}.html", stem.to_string_lossy());
        Path::new(&self.config.output_dir).join(parent).join(output_name)
    }
}

impl ProductDiscovery for MarkdownProcessor {
    fn description(&self) -> &str {
        "Convert Markdown to HTML using markdown"
    }

    fn processor_type(&self) -> ProcessorType {
        ProcessorType::Generator
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        self.should_process() && !file_index.scan(&self.config.scan, true).is_empty()
    }

    fn required_tools(&self) -> Vec<String> {
        vec![self.config.markdown_bin.clone()]
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

        for source in files {
            let output = self.output_path(&source);

            let mut inputs = Vec::with_capacity(1 + extra.len());
            inputs.push(source);
            inputs.extend_from_slice(&extra);

            graph.add_product(inputs, vec![output], crate::processors::names::MARKDOWN, hash.clone())?;
        }

        Ok(())
    }

    fn execute(&self, product: &Product) -> Result<()> {
        let input = product.primary_input();
        let output = product.outputs.first()
            .context("markdown product has no output")?;

        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create markdown output directory: {}", parent.display()))?;
        }

        let mut cmd = Command::new(&self.config.markdown_bin);
        for arg in &self.config.args {
            cmd.arg(arg);
        }
        cmd.arg(input);

        let out = run_command_capture(&mut cmd)?;
        check_command_output(&out, format_args!("markdown {}", input.display()))?;

        fs::write(output, &out.stdout)
            .with_context(|| format!("Failed to write markdown output: {}", output.display()))?;

        Ok(())
    }

    fn clean(&self, product: &Product, verbose: bool) -> Result<usize> {
        clean_outputs(product, crate::processors::names::MARKDOWN, verbose)
    }

    fn config_json(&self) -> Option<String> {
        serde_json::to_string(&self.config).ok()
    }
}
