use anyhow::Result;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::{NpmConfig, output_config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProcessorBase, Processor, SiblingFilter, run_in_anchor_dir, anchor_display_dir, check_command_output};

pub struct NpmProcessor {
    base: ProcessorBase,
    config: NpmConfig,
}

impl NpmProcessor {
    pub fn new(config: NpmConfig) -> Self {
        Self {
            base: ProcessorBase::creator(
                crate::processors::names::NPM,
                "Install Node.js dependencies using npm",
            ),
            config,
        }
    }

    /// Run npm install in the package.json's directory
    fn execute_npm(&self, package_json: &Path) -> Result<()> {
        let mut cmd = Command::new(&self.config.npm);
        cmd.arg(&self.config.command);
        for arg in &self.config.args {
            cmd.arg(arg);
        }
        let output = run_in_anchor_dir(&mut cmd, package_json)?;
        check_command_output(&output, format_args!("npm {} in {}", self.config.command, anchor_display_dir(package_json)))
    }
}

impl Processor for NpmProcessor {
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
        crate::processors::ProcessorBase::clean_output_dir(product, &product.processor, verbose)
    }

    fn required_tools(&self) -> Vec<String> {
        vec![self.config.npm.clone(), "node".to_string()]
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex, instance_name: &str) -> Result<()> {
        let Some(files) = crate::processors::scan_or_skip(&self.config.scan, file_index) else {
            return Ok(());
        };

        let hash = Some(output_config_hash(&self.config, &[]));
        let extra = resolve_extra_inputs(&self.config.dep_inputs)?;

        let siblings = SiblingFilter {
            extensions: &[".json", ".js", ".ts"],
            excludes: &["/.git/", "/out/", "/.rsconstruct/", "/node_modules/"],
        };

        for anchor in files {
            let anchor_dir = anchor.parent().map(|p| p.to_path_buf()).unwrap_or_default();

            let sibling_files = file_index.query(
                &anchor_dir,
                siblings.extensions,
                siblings.excludes,
                &[],
                &[],
                &[],
            );

            let inputs = crate::processors::build_anchor_inputs(&anchor, &sibling_files, &extra);

            if self.config.cache_output_dir {
                let output_dir = if anchor_dir.as_os_str().is_empty() {
                    PathBuf::from("node_modules")
                } else {
                    anchor_dir.join("node_modules")
                };
                graph.add_product_with_output_dir(inputs, vec![], instance_name, hash.clone(), output_dir)?;
            } else {
                graph.add_product(inputs, vec![], instance_name, hash.clone())?;
            }
        }

        Ok(())
    }

    fn supports_batch(&self) -> bool { false }

    fn execute(&self, product: &Product) -> Result<()> {
        self.execute_npm(product.primary_input())
    }
}

fn plugin_create(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(NpmProcessor::new(cfg)))
}
inventory::submit! {
    crate::registry::ProcessorPlugin {
        name: "npm",
        processor_type: crate::processors::ProcessorType::Creator,
        create: plugin_create,
        defconfig_json: crate::registry::default_config_json::<crate::config::NpmConfig>,
        known_fields: crate::registry::typed_known_fields::<crate::config::NpmConfig>,
        output_fields: crate::registry::typed_output_fields::<crate::config::NpmConfig>,
        must_fields: crate::registry::typed_must_fields::<crate::config::NpmConfig>,
        field_descriptions: crate::registry::typed_field_descriptions::<crate::config::NpmConfig>,
    }
}
