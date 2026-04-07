use anyhow::Result;
use std::path::PathBuf;
use std::process::Command;

use crate::config::ExplicitConfig;
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{
    ProcessorBase, ProductDiscovery,
    run_command, check_command_output, ensure_output_dir,
};
use crate::config::output_config_hash;

pub struct ExplicitProcessor {
    base: ProcessorBase,
    config: ExplicitConfig,
}

impl ExplicitProcessor {
    pub fn new(config: ExplicitConfig) -> Self {
        Self {
            base: ProcessorBase::explicit(
                crate::processors::names::EXPLICIT,
                "Run a command with explicitly declared inputs and outputs",
            ),
            config,
        }
    }

    /// Resolve literal inputs. Unlike extra_inputs, missing files are silently
    /// skipped — they may be virtual files from upstream generators that only
    /// appear after fixed-point discovery injects them into the FileIndex.
    fn resolve_inputs(&self, file_index: &FileIndex) -> Vec<PathBuf> {
        let mut resolved = Vec::new();
        // Literal inputs (in config order), only include files that exist
        // or are known to the file index (virtual files from upstream generators)
        for p in &self.config.inputs {
            let path = PathBuf::from(p);
            if path.exists() || file_index.contains(&path) {
                resolved.push(path);
            }
        }
        // Glob inputs (one glob at a time in config order, sorted within each glob)
        for pattern in &self.config.input_globs {
            let mut glob_results: Vec<PathBuf> = Vec::new();
            // Match against real files on disk
            if let Ok(entries) = glob::glob(pattern) {
                for entry in entries.flatten() {
                    if entry.is_file() {
                        glob_results.push(entry);
                    }
                }
            }
            // Also match against virtual files in the file index
            if let Ok(pat) = glob::Pattern::new(pattern) {
                for file in file_index.files() {
                    if pat.matches(&*file.to_string_lossy()) && !glob_results.contains(file) {
                        glob_results.push(file.clone());
                    }
                }
            }
            glob_results.sort();
            glob_results.dedup();
            resolved.extend(glob_results);
        }
        resolved
    }
}

impl ProductDiscovery for ExplicitProcessor {
    fn description(&self) -> &str {
        self.base.description()
    }

    fn processor_type(&self) -> crate::processors::ProcessorType {
        self.base.processor_type()
    }

    fn auto_detect(&self, _file_index: &FileIndex) -> bool {
        // Only active if command is set to something real and outputs are declared
        self.config.command != "true" && !self.config.outputs.is_empty()
    }

    fn required_tools(&self) -> Vec<String> {
        if self.config.command == "true" {
            Vec::new()
        } else {
            vec![self.config.command.clone()]
        }
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex, instance_name: &str) -> Result<()> {
        if self.config.outputs.is_empty() {
            return Ok(());
        }

        let inputs = self.resolve_inputs(file_index);
        if inputs.is_empty() && self.config.inputs.is_empty() && self.config.input_globs.is_empty() {
            return Ok(());
        }

        let outputs: Vec<PathBuf> = self.config.outputs.iter().map(PathBuf::from).collect();
        let hash = Some(output_config_hash(&self.config, &["inputs", "input_globs"]));

        graph.add_product(
            inputs,
            outputs,
            instance_name,
            hash,
        )?;

        Ok(())
    }

    fn execute(&self, product: &Product) -> Result<()> {
        // Ensure output directories exist
        for output in &product.outputs {
            ensure_output_dir(output)?;
        }

        let mut cmd = Command::new(&self.config.command);
        for arg in &self.config.args {
            cmd.arg(arg);
        }
        cmd.arg("--inputs");
        for input in &product.inputs {
            cmd.arg(input);
        }
        cmd.arg("--outputs");
        for output in &product.outputs {
            cmd.arg(output);
        }

        let out = run_command(&mut cmd)?;
        check_command_output(
            &out,
            format_args!("{} ({} inputs → {} outputs)",
                self.config.command,
                product.inputs.len(),
                product.outputs.len(),
            ),
        )
    }

    fn clean(&self, product: &Product, verbose: bool) -> Result<usize> {
        ProcessorBase::clean(product, &product.processor, verbose)
    }

    fn config_json(&self) -> Option<String> {
        ProcessorBase::config_json(&self.config)
    }
}
