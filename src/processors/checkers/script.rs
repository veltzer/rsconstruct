use anyhow::Result;
use std::path::Path;

use crate::config::{ScriptConfig, output_config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProcessorBase, ProductDiscovery, config_file_inputs, run_checker, execute_checker_batch};

pub struct ScriptProcessor {
    base: ProcessorBase,
    config: ScriptConfig,
}

impl ScriptProcessor {
    pub fn new(config: ScriptConfig) -> Self {
        Self {
            base: ProcessorBase::checker(
                crate::processors::names::SCRIPT,
                "Run a user-configured script as a checker",
            ),
            config,
        }
    }

    fn check_files(&self, files: &[&Path]) -> Result<()> {
        let command = self.config.command.as_deref().unwrap();
        run_checker(command, None, &self.config.args, files)
    }
}

impl ProductDiscovery for ScriptProcessor {
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

    fn required_tools(&self) -> Vec<String> {
        self.config.command.iter().cloned().collect()
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex, instance_name: &str) -> Result<()> {
        if self.config.command.is_none() {
            return Ok(());
        }
        let files = file_index.scan(&self.config.scan, true);
        if files.is_empty() {
            return Ok(());
        }
        let hash = Some(output_config_hash(&self.config, &[]));
        let mut dep_inputs = self.config.dep_inputs.clone();
        for ai in &self.config.dep_auto {
            dep_inputs.extend(config_file_inputs(ai));
        }
        // If the command is a local file, depend on its contents
        let command = self.config.command.as_deref().unwrap();
        dep_inputs.extend(config_file_inputs(command));
        let extra = resolve_extra_inputs(&dep_inputs)?;
        for file in files {
            let mut inputs = Vec::with_capacity(1 + extra.len());
            inputs.push(file);
            inputs.extend_from_slice(&extra);
            graph.add_product(inputs, vec![], instance_name, hash.clone())?;
        }
        Ok(())
    }

    fn execute(&self, product: &Product) -> Result<()> {
        self.check_files(&[product.primary_input()])
    }

    fn supports_batch(&self) -> bool {
        self.config.batch
    }

    fn execute_batch(&self, products: &[&Product]) -> Vec<Result<()>> {
        execute_checker_batch(products, |files| self.check_files(files))
    }

}
