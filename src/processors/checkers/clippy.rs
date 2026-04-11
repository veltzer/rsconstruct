use anyhow::Result;
use std::path::Path;
use std::process::Command;

use crate::config::ClippyConfig;
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProcessorBase, Processor, SiblingFilter, DirectoryProductOpts, discover_directory_products, run_in_anchor_dir, anchor_display_dir, check_command_output};

pub struct ClippyProcessor {
    base: ProcessorBase,
    config: ClippyConfig,
}

impl ClippyProcessor {
    pub fn new(config: ClippyConfig) -> Self {
        Self {
            base: ProcessorBase::checker(crate::processors::names::CLIPPY, "Lint Rust projects using Cargo Clippy"),
            config,
        }
    }

    /// Run cargo clippy in the Cargo.toml's directory
    fn execute_clippy(&self, cargo_toml: &Path) -> Result<()> {
        let mut cmd = Command::new(&self.config.cargo);
        cmd.arg(&self.config.standard.command);
        for arg in &self.config.standard.args {
            cmd.arg(arg);
        }
        let output = run_in_anchor_dir(&mut cmd, cargo_toml)?;
        check_command_output(&output, format_args!("cargo {} in {}", self.config.standard.command, anchor_display_dir(cargo_toml)))
    }
}

impl Processor for ClippyProcessor {
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
        vec![self.config.cargo.clone()]
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex, instance_name: &str) -> Result<()> {
        if !crate::processors::scan_root_valid(&self.config.standard.scan) {
            return Ok(());
        }

        discover_directory_products(graph, DirectoryProductOpts {
            scan: &self.config.standard.scan,
            file_index,
            dep_inputs: &self.config.standard.dep_inputs,
            cfg_hash: &self.config,
            siblings: &SiblingFilter {
                extensions: &[".rs", ".toml"],
                excludes: &["/.git/", "/target/", "/.rsconstruct/"],
            },
            processor_name: instance_name,
            output_dir_name: None,
        })
    }

    fn supports_batch(&self) -> bool { false }

    fn execute(&self, product: &Product) -> Result<()> {
        self.execute_clippy(product.primary_input())
    }
}

fn plugin_create(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(ClippyProcessor::new(cfg)))
}
inventory::submit! {
    crate::registry::ProcessorPlugin {
        name: "clippy",
        processor_type: crate::processors::ProcessorType::Checker,
        create: plugin_create,
        defconfig_json: crate::registry::default_config_json::<crate::config::ClippyConfig>,
        known_fields: crate::registry::typed_known_fields::<crate::config::ClippyConfig>,
        output_fields: crate::registry::typed_output_fields::<crate::config::ClippyConfig>,
        must_fields: crate::registry::typed_must_fields::<crate::config::ClippyConfig>,
        field_descriptions: crate::registry::typed_field_descriptions::<crate::config::ClippyConfig>,
    }
}
