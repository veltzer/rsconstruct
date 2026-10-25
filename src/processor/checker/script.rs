use anyhow::Result;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::config::{StandardConfig, output_config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processor::{Processor, config_file_inputs, execute_checker_batch, run_checker};

/// Script processor config. No custom fields.
/// Unused `StandardConfig` fields: formats, `output_dir`.
/// Note: empty command means "no command configured".
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct ScriptConfig {
    #[serde(flatten)]
    pub standard: StandardConfig,
}

pub struct ScriptProcessor {
    config: ScriptConfig,
}

impl ScriptProcessor {
    pub const fn new(config: ScriptConfig) -> Self {
        Self { config }
    }

    fn check_files(&self, ctx: &crate::build_context::BuildContext, files: &[&Path]) -> Result<()> {
        let command = self
            .config
            .standard
            .require_command(crate::processor::names::SCRIPT)?;
        run_checker(
            ctx,
            command,
            None,
            &self.config.standard.args,
            files,
            ctx.max_arg_len(),
        )
    }
}

impl Processor for ScriptProcessor {
    fn scan_config(&self) -> &crate::config::StandardConfig {
        &self.config.standard
    }

    /// One product per scanned file.
    fn discovery(&self) -> crate::processor::Discovery {
        crate::processor::Discovery::PerFile
    }

    fn config_json(&self) -> Option<String> {
        crate::processor::ProcessorBase::config_json(&self.config)
    }

    fn required_tools(&self) -> Vec<String> {
        if self.config.standard.command.is_empty() {
            Vec::new()
        } else {
            vec![self.config.standard.command.clone()]
        }
    }

    fn discover(
        &self,
        graph: &mut BuildGraph,
        file_index: &FileIndex,
        instance_name: &str,
    ) -> Result<()> {
        if self.config.standard.command.is_empty() {
            return Ok(());
        }
        let files = file_index.scan(&self.config.standard, true);
        if files.is_empty() {
            return Ok(());
        }
        let hash = Some(output_config_hash(
            &self.config,
            &crate::config::checksum_fields_of(instance_name),
        ));
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

    fn execute_batch(
        &self,
        ctx: &crate::build_context::BuildContext,
        products: &[&Product],
    ) -> Vec<Result<()>> {
        execute_checker_batch(ctx, products, |ctx, files| self.check_files(ctx, files))
    }
}

fn plugin_create(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| Box::new(ScriptProcessor::new(cfg)))
}
inventory::submit! {
    crate::registries::ProcessorPlugin {
        version: 1,
        name: "script",
        processor_type: crate::processor::ProcessorType::Checker,
        create: plugin_create,
        fields: &[
            // required: false is deliberate — a bare [processor.script]
            // section is legal and simply discovers nothing (pinned by
            // script_no_project_discovered); execute() errors if a product
            // exists without a command.
            crate::config::FieldSpec { name: "command", ty: crate::config::FieldType::String,
                affects_output: true, required: false,
                doc: "Path to the checker tool executable" },
        ],
        omit_standard_fields: &[],
        scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &[], src_exclude_dirs: &[] }),
        defaults: None,
        defconfig_json: crate::registries::default_config_json::<ScriptConfig>,
        keywords: &["shell", "script", "checker", "sh", "bash"],
        description: "Run a user-configured script as a checker",
        is_builtin: false,
        is_rust: false,
        supports_batch: true,
        caches_outputs: true,
        max_jobs_cap: None,
    }
}
