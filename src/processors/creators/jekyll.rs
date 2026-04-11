use anyhow::Result;
use std::path::Path;
use std::process::Command;

use crate::config::JekyllConfig;
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProcessorBase, Processor, SiblingFilter, DirectoryProductOpts, discover_directory_products, scan_root_valid, run_in_anchor_dir, anchor_display_dir, check_command_output};

pub struct JekyllProcessor {
    base: ProcessorBase,
    config: JekyllConfig,
}

impl JekyllProcessor {
    pub fn new(config: JekyllConfig) -> Self {
        Self {
            base: ProcessorBase::creator(
                crate::processors::names::JEKYLL,
                "Build Jekyll sites",
            ),
            config,
        }
    }

    fn should_process(&self) -> bool {
        scan_root_valid(&self.config.standard.scan)
    }

    fn execute_jekyll(&self, config_yml: &Path) -> Result<()> {
        let mut cmd = Command::new("jekyll");
        cmd.arg("build");
        for arg in &self.config.standard.args {
            cmd.arg(arg);
        }
        let output = run_in_anchor_dir(&mut cmd, config_yml)?;
        check_command_output(&output, format_args!("jekyll build in {}", anchor_display_dir(config_yml)))
    }
}

impl Processor for JekyllProcessor {
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

    fn config_json(&self) -> Option<String> {
        crate::processors::ProcessorBase::config_json(&self.config)
    }


    fn clean(&self, product: &crate::graph::Product, verbose: bool) -> anyhow::Result<usize> {
        crate::processors::ProcessorBase::clean_output_dir(product, &product.processor, verbose)
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        self.should_process() && !file_index.scan(&self.config.standard.scan, true).is_empty()
    }

    fn required_tools(&self) -> Vec<String> {
        vec!["jekyll".to_string(), "ruby".to_string()]
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex, instance_name: &str) -> Result<()> {
        if !self.should_process() {
            return Ok(());
        }

        discover_directory_products(graph, DirectoryProductOpts {
            scan: &self.config.standard.scan,
            file_index,
            dep_inputs: &self.config.standard.dep_inputs,
            cfg_hash: &self.config,
            siblings: &SiblingFilter {
                extensions: &[""],
                excludes: &["/.git/", "/out/", "/.rsconstruct/", "/_site/"],
            },
            processor_name: instance_name,
            output_dir_name: Some("_site"),
        })
    }

    fn supports_batch(&self) -> bool { false }

    fn execute(&self, product: &Product) -> Result<()> {
        self.execute_jekyll(product.primary_input())
    }
}

fn plugin_create(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(JekyllProcessor::new(cfg)))
}
inventory::submit! {
    crate::registry::ProcessorPlugin {
        name: "jekyll",
        processor_type: crate::processors::ProcessorType::Creator,
        create: plugin_create,
        defconfig_json: crate::registry::default_config_json::<crate::config::JekyllConfig>,
        known_fields: crate::registry::typed_known_fields::<crate::config::JekyllConfig>,
        output_fields: crate::registry::typed_output_fields::<crate::config::JekyllConfig>,
        must_fields: crate::registry::typed_must_fields::<crate::config::JekyllConfig>,
        field_descriptions: crate::registry::typed_field_descriptions::<crate::config::JekyllConfig>,
    }
}
