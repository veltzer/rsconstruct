use anyhow::Result;
use std::path::Path;
use std::process::Command;

use crate::config::JekyllConfig;
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProductDiscovery, ProcessorType, SiblingFilter, DirectoryProductOpts, discover_directory_products, scan_root_valid, run_in_anchor_dir, anchor_display_dir, check_command_output};

pub struct JekyllProcessor {
    config: JekyllConfig,
}

impl JekyllProcessor {
    pub fn new(config: JekyllConfig) -> Self {
        Self { config }
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
    fn description(&self) -> &str {
        "Build Jekyll sites"
    }

    fn processor_type(&self) -> ProcessorType {
        ProcessorType::MassGenerator
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        self.should_process() && !file_index.scan(&self.config.scan, true).is_empty()
    }

    fn required_tools(&self) -> Vec<String> {
        vec!["jekyll".to_string(), "ruby".to_string()]
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex) -> Result<()> {
        if !self.should_process() {
            return Ok(());
        }

        discover_directory_products(graph, DirectoryProductOpts {
            scan: &self.config.scan,
            file_index,
            extra_inputs: &self.config.extra_inputs,
            cfg_hash: &self.config,
            siblings: &SiblingFilter {
                extensions: &[""],
                excludes: &["/.git/", "/out/", "/.rsconstruct/", "/_site/"],
            },
            processor_name: crate::processors::names::JEKYLL,
            output_dir_name: Some("_site"),
        })
    }

    fn config_json(&self) -> Option<String> {
        serde_json::to_string(&self.config).ok()
    }

    fn execute(&self, product: &Product) -> Result<()> {
        self.execute_jekyll(product.primary_input())
    }

    fn max_jobs(&self) -> Option<usize> {
        self.config.max_jobs
    }
}
