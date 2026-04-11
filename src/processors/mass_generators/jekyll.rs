use anyhow::Result;
use std::path::Path;
use std::process::Command;

use crate::config::JekyllConfig;
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProcessorBase, ProductDiscovery, SiblingFilter, DirectoryProductOpts, discover_directory_products, scan_root_valid, run_in_anchor_dir, anchor_display_dir, check_command_output};

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
        scan_root_valid(&self.config.scan)
    }

    fn execute_jekyll(&self, config_yml: &Path) -> Result<()> {
        let mut cmd = Command::new("jekyll");
        cmd.arg("build");
        for arg in &self.config.args {
            cmd.arg(arg);
        }
        let output = run_in_anchor_dir(&mut cmd, config_yml)?;
        check_command_output(&output, format_args!("jekyll build in {}", anchor_display_dir(config_yml)))
    }
}

impl ProductDiscovery for JekyllProcessor {
    fn scan_config(&self) -> &crate::config::ScanConfig {
        &self.config.scan
    }

    fn standard_config(&self) -> Option<&crate::config::StandardConfig> {
        Some(&self.config)
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
        self.should_process() && !file_index.scan(&self.config.scan, true).is_empty()
    }

    fn required_tools(&self) -> Vec<String> {
        vec!["jekyll".to_string(), "ruby".to_string()]
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex, instance_name: &str) -> Result<()> {
        if !self.should_process() {
            return Ok(());
        }

        discover_directory_products(graph, DirectoryProductOpts {
            scan: &self.config.scan,
            file_index,
            dep_inputs: &self.config.dep_inputs,
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

inventory::submit! {
    &crate::registry::typed_plugin::<crate::config::JekyllConfig>(
        "jekyll", |cfg| Box::new(JekyllProcessor::new(cfg))
    ) as &dyn crate::registry::RegistryOps
}
