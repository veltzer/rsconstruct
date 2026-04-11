use anyhow::Result;
use std::path::Path;
use std::process::Command;

use crate::config::GeneratorConfig;
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{
    ProcessorBase, Processor, scan_root_valid,
    run_command, check_command_output, execute_generator_batch,
    config_file_inputs,
};
use crate::config::{output_config_hash, resolve_extra_inputs};

pub struct GeneratorProcessor {
    base: ProcessorBase,
    config: GeneratorConfig,
}

impl GeneratorProcessor {
    pub fn new(config: GeneratorConfig) -> Self {
        Self {
            base: ProcessorBase::generator(
                crate::processors::names::GENERATOR,
                "Run a user-configured script as a generator",
            ),
            config,
        }
    }

    fn should_process(&self) -> bool {
        scan_root_valid(&self.config.scan) && self.config.command.is_some()
    }

    fn execute_product(&self, product: &Product) -> Result<()> {
        self.run_pairs(&[(product.primary_input(), product.primary_output())])
    }

    fn run_pairs(&self, pairs: &[(&Path, &Path)]) -> Result<()> {
        for pair in pairs {
            crate::processors::ensure_output_dir(pair.1)?;
        }

        let command = self.config.command.as_deref().unwrap();
        let mut cmd = Command::new(command);
        for arg in &self.config.args {
            cmd.arg(arg);
        }
        for (input, output) in pairs {
            cmd.arg(input);
            cmd.arg(output);
        }

        let out = run_command(&mut cmd)?;
        check_command_output(&out, format_args!("{} ({} file(s))", command, pairs.len()))
    }
}

impl Processor for GeneratorProcessor {
    fn scan_config(&self) -> &crate::config::ScanConfig {
        &self.config.scan
    }


    fn description(&self) -> &str {
        self.base.description()
    }

    fn processor_type(&self) -> crate::processors::ProcessorType {
        self.base.processor_type()
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

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        self.should_process() && !file_index.scan(&self.config.scan, true).is_empty()
    }

    fn required_tools(&self) -> Vec<String> {
        match &self.config.command {
            Some(cmd) => vec![cmd.clone()],
            None => vec![],
        }
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex, instance_name: &str) -> Result<()> {
        if !self.should_process() {
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
        let src_dirs = self.config.scan.src_dirs();

        for source in &files {
            let output = super::output_path(
                source, src_dirs, &self.config.output_dir, &self.config.output_extension,
            );
            let mut inputs = Vec::with_capacity(1 + extra.len());
            inputs.push(source.clone());
            inputs.extend_from_slice(&extra);
            graph.add_product(inputs, vec![output], instance_name, hash.clone())?;
        }
        Ok(())
    }

    fn execute(&self, product: &Product) -> Result<()> {
        self.execute_product(product)
    }

    fn supports_batch(&self) -> bool {
        self.config.batch
    }

    fn execute_batch(&self, products: &[&Product]) -> Vec<Result<()>> {
        execute_generator_batch(products, |pairs| self.run_pairs(pairs))
    }
}

fn plugin_create(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(GeneratorProcessor::new(cfg)))
}
inventory::submit! {
    crate::registry::ProcessorPlugin {
        name: "generator",
        processor_type: crate::processors::ProcessorType::Generator,
        create: plugin_create,
        defconfig_json: crate::registry::default_config_json::<crate::config::GeneratorConfig>,
        known_fields: crate::registry::typed_known_fields::<crate::config::GeneratorConfig>,
        output_fields: crate::registry::typed_output_fields::<crate::config::GeneratorConfig>,
        must_fields: crate::registry::typed_must_fields::<crate::config::GeneratorConfig>,
        field_descriptions: crate::registry::typed_field_descriptions::<crate::config::GeneratorConfig>,
    }
}
