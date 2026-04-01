use anyhow::Result;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::{RustSingleFileConfig, output_config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProductDiscovery, clean_outputs, run_command, check_command_output, scan_root_valid};

pub struct RustSingleFileProcessor {
    config: RustSingleFileConfig,
}

impl RustSingleFileProcessor {
    pub fn new(config: RustSingleFileConfig) -> Self {
        Self { config }
    }

    fn get_output_path(&self, source: &Path) -> PathBuf {
        let scan_dirs = self.config.scan.scan_dirs();
        let full_parent = source.parent().unwrap_or(Path::new(""));
        let parent = scan_dirs.iter()
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
        "Compile single-file Rust programs into executables"
    }

    fn processor_type(&self) -> crate::processors::ProcessorType {
        crate::processors::ProcessorType::Generator
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        scan_root_valid(&self.config.scan)
            && !file_index.scan(&self.config.scan, true).is_empty()
    }

    fn required_tools(&self) -> Vec<String> {
        vec![self.config.rustc.clone()]
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex) -> Result<()> {
        let files = file_index.scan(&self.config.scan, true);
        if files.is_empty() {
            return Ok(());
        }

        let hash = Some(output_config_hash(&self.config, &[]));
        let extra = resolve_extra_inputs(&self.config.extra_inputs)?;

        for source in &files {
            let output = self.get_output_path(source);

            let mut inputs = Vec::with_capacity(1 + extra.len());
            inputs.push(source.clone());
            inputs.extend_from_slice(&extra);

            graph.add_product(
                inputs,
                vec![output],
                crate::processors::names::RUST_SINGLE_FILE,
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

    fn clean(&self, product: &Product, verbose: bool) -> Result<usize> {
        clean_outputs(product, crate::processors::names::RUST_SINGLE_FILE, verbose)
    }

    fn config_json(&self) -> Option<String> {
        serde_json::to_string(&self.config).ok()
    }
}
