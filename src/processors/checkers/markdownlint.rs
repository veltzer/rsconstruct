use anyhow::Result;
use std::path::PathBuf;
use std::process::Command;

use crate::config::{MarkdownlintConfig, output_config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProcessorBase, Processor, check_command_output, config_file_inputs, run_command};

pub struct MarkdownlintProcessor {
    base: ProcessorBase,
    config: MarkdownlintConfig,
}

impl MarkdownlintProcessor {
    pub fn new(config: MarkdownlintConfig) -> Self {
        Self {
            base: ProcessorBase::checker(crate::processors::names::MARKDOWNLINT, "Lint Markdown files using markdownlint"),
            config,
        }
    }
}

impl Processor for MarkdownlintProcessor {
    fn scan_config(&self) -> &crate::config::StandardConfig {
        &self.config.standard
    }

    fn standard_config(&self) -> Option<&crate::config::StandardConfig> {
        Some(&self.config.standard)
    }

    fn description(&self) -> &str {
        self.base.description()
    }

    fn processor_type(&self) -> crate::processors::ProcessorType {
        self.base.processor_type()
    }

    fn required_tools(&self) -> Vec<String> {
        vec![self.config.markdownlint_bin.clone()]
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex, instance_name: &str) -> Result<()> {
        let files = file_index.scan(&self.config.standard, true);
        if files.is_empty() {
            return Ok(());
        }
        let hash = Some(output_config_hash(&self.config, &[]));
        let mut dep_inputs = self.config.standard.dep_inputs.clone();
        for ai in &self.config.standard.dep_auto {
            dep_inputs.extend(config_file_inputs(ai));
        }
        let extra = resolve_extra_inputs(&dep_inputs)?;

        // Only depend on the npm stamp when using a local repo install
        let npm_stamp = if self.config.local_repo {
            Some(PathBuf::from(&self.config.npm_stamp))
        } else {
            None
        };

        for file in files {
            let mut inputs = Vec::with_capacity(1 + extra.len() + 1);
            inputs.push(file);
            inputs.extend_from_slice(&extra);
            if let Some(ref stamp) = npm_stamp {
                inputs.push(stamp.clone());
            }
            graph.add_product(inputs, vec![], instance_name, hash.clone())?;
        }
        Ok(())
    }

    fn supports_batch(&self) -> bool { false }

    fn execute(&self, product: &Product) -> Result<()> {
        let file = product.primary_input();
        let mut cmd = Command::new(&self.config.markdownlint_bin);
        for arg in &self.config.standard.args {
            cmd.arg(arg);
        }
        cmd.arg(file);
        let output = run_command(&mut cmd)?;
        check_command_output(&output, format_args!("markdownlint {}", file.display()))
    }
}

fn plugin_create(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(MarkdownlintProcessor::new(cfg)))
}
inventory::submit! {
    crate::registry::ProcessorPlugin {
        name: "markdownlint",
        processor_type: crate::processors::ProcessorType::Checker,
        create: plugin_create,
        defconfig_json: crate::registry::default_config_json::<crate::config::MarkdownlintConfig>,
        known_fields: crate::registry::typed_known_fields::<crate::config::MarkdownlintConfig>,
        output_fields: crate::registry::typed_output_fields::<crate::config::MarkdownlintConfig>,
        must_fields: crate::registry::typed_must_fields::<crate::config::MarkdownlintConfig>,
        field_descriptions: crate::registry::typed_field_descriptions::<crate::config::MarkdownlintConfig>,
    }
}
