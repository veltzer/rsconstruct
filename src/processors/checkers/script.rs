use anyhow::Result;
use std::path::Path;

use crate::config::{ScriptConfig, output_config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProcessorBase, Processor, config_file_inputs, run_checker, execute_checker_batch};

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

    fn check_files(&self, ctx: &crate::build_context::BuildContext, files: &[&Path]) -> Result<()> {
        let command = self.config.standard.require_command(crate::processors::names::SCRIPT)?;
        run_checker(ctx, command, None, &self.config.standard.args, files)
    }
}

impl Processor for ScriptProcessor {
    fn scan_config(&self) -> &crate::config::StandardConfig {
        &self.config.standard
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
        self.config.standard.max_jobs
    }

    fn required_tools(&self) -> Vec<String> {
        if self.config.standard.command.is_empty() {
            Vec::new()
        } else {
            vec![self.config.standard.command.clone()]
        }
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex, instance_name: &str) -> Result<()> {
        if self.config.standard.command.is_empty() {
            return Ok(());
        }
        let files = file_index.scan(&self.config.standard, true);
        if files.is_empty() {
            return Ok(());
        }
        let hash = Some(output_config_hash(&self.config, &[]));
        let mut dep_inputs = self.config.standard.dep_inputs.clone();
        for ai in &self.config.standard.dep_auto {
            dep_inputs.extend(config_file_inputs(ai));
        }
        // If the command is a local file, depend on its contents
        dep_inputs.extend(config_file_inputs(&self.config.standard.command));
        let extra = resolve_extra_inputs(&dep_inputs)?;
        for file in files {
            let mut inputs = Vec::with_capacity(1 + extra.len());
            inputs.push(file);
            inputs.extend_from_slice(&extra);
            graph.add_product(inputs, vec![], instance_name, hash.clone())?;
        }
        Ok(())
    }

    fn execute(&self, ctx: &crate::build_context::BuildContext, product: &Product) -> Result<()> {
        self.check_files(ctx, &[product.primary_input()])
    }

    fn supports_batch(&self) -> bool {
        self.config.standard.batch
    }

    fn execute_batch(&self, ctx: &crate::build_context::BuildContext, products: &[&Product]) -> Vec<Result<()>> {
        execute_checker_batch(ctx, products, |ctx, files| self.check_files(ctx, files))
    }

}

fn plugin_create(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| Box::new(ScriptProcessor::new(cfg)))
}
inventory::submit! {
    crate::registries::ProcessorPlugin {
        version: 1,
        name: "script",
        processor_type: crate::processors::ProcessorType::Checker,
        create: plugin_create,
        defconfig_json: crate::registries::default_config_json::<crate::config::ScriptConfig>,
        known_fields: crate::registries::typed_known_fields::<crate::config::ScriptConfig>,
        output_fields: crate::registries::typed_output_fields::<crate::config::ScriptConfig>,
        must_fields: crate::registries::typed_must_fields::<crate::config::ScriptConfig>,
        field_descriptions: crate::registries::typed_field_descriptions::<crate::config::ScriptConfig>,
    }
}
