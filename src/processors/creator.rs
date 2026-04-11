use std::path::PathBuf;
use std::process::Command;
use anyhow::Result;

use crate::config::{CreatorConfig, output_config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProcessorBase, Processor, ProcessorType,
    run_in_anchor_dir, anchor_display_dir, check_command_output};

/// A data-driven processor that runs a command and caches declared outputs.
///
/// Unlike generators (1 input → 1 output) or checkers (validate only),
/// a Creator runs a command that may produce any combination of files and
/// directories. The user declares what outputs to cache in the config.
///
/// Example config:
/// ```toml
/// [processor.creator.pip]
/// command = "pip"
/// args = ["install", "-r", "requirements.txt"]
/// output_dirs = [".venv"]
/// src_extensions = ["requirements.txt"]
/// ```
pub struct CreatorProcessor {
    base: ProcessorBase,
    config: CreatorConfig,
}

impl CreatorProcessor {
    pub fn new(config: CreatorConfig) -> Self {
        Self {
            base: ProcessorBase::creator("creator", "Run a command and cache declared outputs"),
            config,
        }
    }
}

impl Processor for CreatorProcessor {
    fn scan_config(&self) -> &crate::config::ScanConfig {
        &self.config.standard.scan
    }

    fn description(&self) -> &str {
        self.base.description()
    }

    fn processor_type(&self) -> ProcessorType {
        self.base.processor_type()
    }

    fn config_json(&self) -> Option<String> {
        ProcessorBase::config_json(&self.config)
    }

    fn max_jobs(&self) -> Option<usize> {
        self.config.standard.max_jobs
    }

    fn clean(&self, product: &Product, verbose: bool) -> Result<usize> {
        ProcessorBase::clean_output_dir(product, &product.processor, verbose)
    }

    fn required_tools(&self) -> Vec<String> {
        if self.config.standard.command.is_empty() {
            Vec::new()
        } else {
            vec![self.config.standard.command.clone()]
        }
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex, instance_name: &str) -> Result<()> {
        let Some(files) = crate::processors::scan_or_skip(&self.config.standard.scan, file_index) else {
            return Ok(());
        };

        let hash = Some(output_config_hash(&self.config, &["output_dirs", "output_files"]));
        let extra = resolve_extra_inputs(&self.config.standard.dep_inputs)?;

        for anchor in files {
            let anchor_dir = anchor.parent().map(|p| p.to_path_buf()).unwrap_or_default();

            let mut inputs = Vec::with_capacity(1 + extra.len());
            inputs.push(anchor.clone());
            inputs.extend_from_slice(&extra);

            let resolve = |rel: &str| -> PathBuf {
                if anchor_dir.as_os_str().is_empty() {
                    PathBuf::from(rel)
                } else {
                    anchor_dir.join(rel)
                }
            };

            let output_files: Vec<PathBuf> = self.config.output_files.iter()
                .map(|f| resolve(f))
                .collect();

            let output_dirs: Vec<PathBuf> = self.config.output_dirs.iter()
                .map(|d| resolve(d))
                .collect();

            if output_dirs.is_empty() {
                graph.add_product(inputs, output_files, instance_name, hash.clone())?;
            } else {
                graph.add_product_with_output_dirs_and_variant(
                    inputs, output_files, instance_name, hash.clone(), output_dirs, None,
                )?;
            }
        }

        Ok(())
    }

    fn supports_batch(&self) -> bool { false }

    fn execute(&self, product: &Product) -> Result<()> {
        let anchor = product.primary_input();
        let mut cmd = Command::new(&self.config.standard.command);
        for arg in &self.config.standard.args {
            cmd.arg(arg);
        }
        let output = run_in_anchor_dir(&mut cmd, anchor)?;
        check_command_output(&output, format_args!("{} in {}", self.config.standard.command, anchor_display_dir(anchor)))
    }
}

fn plugin_create(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(CreatorProcessor::new(cfg)))
}
inventory::submit! {
    crate::registry::ProcessorPlugin {
        name: "creator",
        processor_type: crate::processors::ProcessorType::Creator,
        create: plugin_create,
        defconfig_json: crate::registry::default_config_json::<crate::config::CreatorConfig>,
        known_fields: crate::registry::typed_known_fields::<crate::config::CreatorConfig>,
        output_fields: crate::registry::typed_output_fields::<crate::config::CreatorConfig>,
        must_fields: crate::registry::typed_must_fields::<crate::config::CreatorConfig>,
        field_descriptions: crate::registry::typed_field_descriptions::<crate::config::CreatorConfig>,
    }
}
