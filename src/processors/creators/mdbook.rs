use anyhow::Result;
use std::path::Path;
use std::process::Command;

use crate::config::MdbookConfig;
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProcessorBase, Processor, SiblingFilter, DirectoryProductOpts, discover_directory_products, scan_root_valid, run_in_anchor_dir, anchor_display_dir, check_command_output};

pub struct MdbookProcessor {
    base: ProcessorBase,
    config: MdbookConfig,
}

impl MdbookProcessor {
    pub fn new(config: MdbookConfig) -> Self {
        Self {
            base: ProcessorBase::creator(
                crate::processors::names::MDBOOK,
                "Build mdbook documentation",
            ),
            config,
        }
    }

    /// Run mdbook build in the book.toml's directory
    fn execute_mdbook(&self, book_toml: &Path) -> Result<()> {
        let mut cmd = Command::new(&self.config.mdbook);
        cmd.arg("build");
        cmd.arg(".");
        for arg in &self.config.args {
            cmd.arg(arg);
        }
        let output = run_in_anchor_dir(&mut cmd, book_toml)?;
        check_command_output(&output, format_args!("mdbook build in {}", anchor_display_dir(book_toml)))
    }
}

impl Processor for MdbookProcessor {
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
        vec![self.config.mdbook.clone()]
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex, instance_name: &str) -> Result<()> {
        if !scan_root_valid(&self.config.scan) {
            return Ok(());
        }

        discover_directory_products(graph, DirectoryProductOpts {
            scan: &self.config.scan,
            file_index,
            dep_inputs: &self.config.dep_inputs,
            cfg_hash: &self.config,
            siblings: &SiblingFilter {
                extensions: &[".md", ".toml"],
                excludes: &["/.git/", "/out/", "/.rsconstruct/", "/book/"],
            },
            processor_name: instance_name,
            output_dir_name: if self.config.cache_output_dir {
                Some(self.config.output_dir.as_str())
            } else {
                None
            },
        })
    }

    fn supports_batch(&self) -> bool { false }

    fn execute(&self, product: &Product) -> Result<()> {
        self.execute_mdbook(product.primary_input())
    }
}

fn plugin_create(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(MdbookProcessor::new(cfg)))
}
inventory::submit! {
    crate::registry::ProcessorPlugin {
        name: "mdbook",
        processor_type: crate::processors::ProcessorType::Creator,
        create: plugin_create,
        defconfig_json: crate::registry::default_config_json::<crate::config::MdbookConfig>,
        known_fields: crate::registry::typed_known_fields::<crate::config::MdbookConfig>,
        output_fields: crate::registry::typed_output_fields::<crate::config::MdbookConfig>,
        must_fields: crate::registry::typed_must_fields::<crate::config::MdbookConfig>,
        field_descriptions: crate::registry::typed_field_descriptions::<crate::config::MdbookConfig>,
    }
}
