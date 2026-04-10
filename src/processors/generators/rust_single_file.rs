use anyhow::Result;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::{RustSingleFileConfig, output_config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProcessorBase, ProductDiscovery, run_command, check_command_output};

pub struct RustSingleFileProcessor {
    base: ProcessorBase,
    config: RustSingleFileConfig,
}

impl RustSingleFileProcessor {
    pub fn new(config: RustSingleFileConfig) -> Self {
        Self {
            base: ProcessorBase::generator(
                crate::processors::names::RUST_SINGLE_FILE,
                "Compile single-file Rust programs into executables",
            ),
            config,
        }
    }

    fn get_output_path(&self, source: &Path) -> PathBuf {
        let src_dirs = self.config.scan.src_dirs();
        let full_parent = source.parent().unwrap_or(Path::new(""));
        let parent = src_dirs.iter()
            .filter(|d| !d.is_empty())
            .find_map(|d| full_parent.strip_prefix(d).ok())
            .unwrap_or(full_parent);
        let stem = source.file_stem().unwrap_or_default();
        let output_name = format!("{}{}", stem.to_string_lossy(), self.config.output_suffix);
        Path::new(&self.config.output_dir).join(parent).join(output_name)
    }
}

impl ProductDiscovery for RustSingleFileProcessor {
    fn description(&self) -> &str {
        self.base.description()
    }

    fn processor_type(&self) -> crate::processors::ProcessorType {
        self.base.processor_type()
    }

    fn auto_detect(&self, file_index: &crate::file_index::FileIndex) -> bool {
        crate::processors::ProcessorBase::auto_detect(&self.config.scan, file_index)
    }

    fn config_json(&self) -> Option<String> {
        crate::processors::ProcessorBase::config_json(&self.config)
    }

    fn max_jobs(&self) -> Option<usize> {
        self.config.max_jobs
    }

    fn clean(&self, product: &crate::graph::Product, verbose: bool) -> anyhow::Result<usize> {
        crate::processors::ProcessorBase::clean(product, &product.processor, verbose)
    }

    fn required_tools(&self) -> Vec<String> {
        vec![self.config.rustc.clone()]
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex, instance_name: &str) -> Result<()> {
        let files = file_index.scan(&self.config.scan, true);
        if files.is_empty() {
            return Ok(());
        }

        let hash = Some(output_config_hash(&self.config, &[]));
        let extra = resolve_extra_inputs(&self.config.dep_inputs)?;

        for source in &files {
            let output = self.get_output_path(source);

            let mut inputs = Vec::with_capacity(1 + extra.len());
            inputs.push(source.clone());
            inputs.extend_from_slice(&extra);

            graph.add_product(
                inputs,
                vec![output],
                instance_name,
                hash.clone(),
            )?;
        }

        Ok(())
    }

    fn execute(&self, product: &Product) -> Result<()> {
        let source = product.primary_input();
        let output = product.primary_output();

        crate::processors::ensure_output_dir(output)?;

        let mut cmd = Command::new(&self.config.rustc);
        for flag in &self.config.flags {
            cmd.arg(flag);
        }
        cmd.arg("-o").arg(output).arg(source);

        let out = run_command(&mut cmd)?;
        check_command_output(&out, format_args!("rustc {}", source.display()))
    }

}
