use anyhow::Result;
use std::path::Path;
use std::process::Command;

use crate::config::GeneratorConfig;
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{
    ProductDiscovery, ProcessorType, clean_outputs, scan_root_valid,
    run_command, check_command_output, execute_generator_batch,
    config_file_inputs,
};
use crate::config::{output_config_hash, resolve_extra_inputs};

pub struct GeneratorProcessor {
    config: GeneratorConfig,
}

impl GeneratorProcessor {
    pub fn new(config: GeneratorConfig) -> Self {
        Self { config }
    }

    fn should_process(&self) -> bool {
        scan_root_valid(&self.config.scan) && !self.config.command.is_empty()
    }

    fn execute_product(&self, product: &Product) -> Result<()> {
        self.run_pairs(&[(product.primary_input(), product.primary_output())])
    }

    fn run_pairs(&self, pairs: &[(&Path, &Path)]) -> Result<()> {
        for pair in pairs {
            crate::processors::ensure_output_dir(pair.1)?;
        }

        let mut cmd = Command::new(&self.config.command);
        for arg in &self.config.args {
            cmd.arg(arg);
        }
        for (input, output) in pairs {
            cmd.arg(input);
            cmd.arg(output);
        }

        let out = run_command(&mut cmd)?;
        check_command_output(&out, format_args!("{} ({} file(s))", self.config.command, pairs.len()))
    }
}

impl ProductDiscovery for GeneratorProcessor {
    fn description(&self) -> &str {
        "Run a user-configured script as a generator"
    }

    fn processor_type(&self) -> ProcessorType {
        ProcessorType::Generator
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        self.should_process() && !file_index.scan(&self.config.scan, true).is_empty()
    }

    fn required_tools(&self) -> Vec<String> {
        vec![self.config.command.clone()]
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex) -> Result<()> {
        if !self.should_process() {
            return Ok(());
        }
        let files = file_index.scan(&self.config.scan, true);
        if files.is_empty() {
            return Ok(());
        }

        let hash = Some(output_config_hash(&self.config, &[]));
        let mut extra_inputs = self.config.extra_inputs.clone();
        for ai in &self.config.auto_inputs {
            extra_inputs.extend(config_file_inputs(ai));
        }
        let extra = resolve_extra_inputs(&extra_inputs)?;
        let scan_dirs = self.config.scan.scan_dirs();

        for source in &files {
            let output = super::output_path(
                source, scan_dirs, &self.config.output_dir, &self.config.output_extension,
            );
            let mut inputs = Vec::with_capacity(1 + extra.len());
            inputs.push(source.clone());
            inputs.extend_from_slice(&extra);
            graph.add_product(inputs, vec![output], crate::processors::names::GENERATOR, hash.clone())?;
        }
        Ok(())
    }

    fn execute(&self, product: &Product) -> Result<()> {
        self.execute_product(product)
    }

    fn clean(&self, product: &Product, verbose: bool) -> Result<usize> {
        clean_outputs(product, crate::processors::names::GENERATOR, verbose)
    }

    fn supports_batch(&self) -> bool {
        self.config.batch
    }

    fn execute_batch(&self, products: &[&Product]) -> Vec<Result<()>> {
        execute_generator_batch(products, |pairs| self.run_pairs(pairs))
    }

    fn config_json(&self) -> Option<String> {
        serde_json::to_string(&self.config).ok()
    }
}
