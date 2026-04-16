use anyhow::Result;
use std::path::Path;
use std::process::Command;

use crate::config::MakeConfig;
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProcessorBase, Processor, SiblingFilter, DirectoryProductOpts, discover_directory_products, run_in_anchor_dir, anchor_display_dir, check_command_output};

pub struct MakeProcessor {
    base: ProcessorBase,
    config: MakeConfig,
}

impl MakeProcessor {
    pub fn new(config: MakeConfig) -> Self {
        Self {
            base: ProcessorBase::checker(crate::processors::names::MAKE, "Run make in directories containing Makefiles"),
            config,
        }
    }

    /// Run make in the Makefile's directory
    fn execute_make(&self, ctx: &crate::build_context::BuildContext, makefile: &Path) -> Result<()> {
        let mut cmd = Command::new(&self.config.standard.command);
        for arg in &self.config.standard.args {
            cmd.arg(arg);
        }
        if !self.config.target.is_empty() {
            cmd.arg(&self.config.target);
        }
        let output = run_in_anchor_dir(ctx, &mut cmd, makefile)?;
        check_command_output(&output, format_args!("make in {}", anchor_display_dir(makefile)))
    }
}

impl Processor for MakeProcessor {
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
        vec![self.config.standard.command.clone()]
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex, instance_name: &str) -> Result<()> {
        if !crate::processors::scan_root_valid(&self.config.standard) {
            return Ok(());
        }

        discover_directory_products(graph, DirectoryProductOpts {
            scan: &self.config.standard,
            file_index,
            dep_inputs: &self.config.standard.dep_inputs,
            cfg_hash: &self.config,
            siblings: &SiblingFilter {
                extensions: &[""],
                excludes: &["/.git/", "/out/", "/.rsconstruct/"],
            },
            processor_name: instance_name,
            output_dir_name: None,
        })
    }

    fn supports_batch(&self) -> bool { false }

    fn execute(&self, ctx: &crate::build_context::BuildContext, product: &Product) -> Result<()> {
        self.execute_make(ctx, product.primary_input())
    }
}

fn plugin_create(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| Box::new(MakeProcessor::new(cfg)))
}
inventory::submit! {
    crate::registries::ProcessorPlugin {
        version: 1,
        name: "make",
        processor_type: crate::processors::ProcessorType::Checker,
        create: plugin_create,
        defconfig_json: crate::registries::default_config_json::<crate::config::MakeConfig>,
        known_fields: crate::registries::typed_known_fields::<crate::config::MakeConfig>,
        output_fields: crate::registries::typed_output_fields::<crate::config::MakeConfig>,
        must_fields: crate::registries::typed_must_fields::<crate::config::MakeConfig>,
        field_descriptions: crate::registries::typed_field_descriptions::<crate::config::MakeConfig>,
        keywords: &["make", "makefile", "builder", "checker"],
        description: "Run make in directories containing Makefiles",
        is_native: false,
        can_fix: false,
    }
}
