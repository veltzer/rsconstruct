use anyhow::Result;
use std::path::Path;
use std::process::Command;

use crate::config::ClippyConfig;
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProcessorBase, ProductDiscovery, SiblingFilter, DirectoryProductOpts, discover_directory_products, run_in_anchor_dir, anchor_display_dir, check_command_output};

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
        cmd.arg(&self.config.command);
        for arg in &self.config.args {
            cmd.arg(arg);
        }
        let output = run_in_anchor_dir(&mut cmd, cargo_toml)?;
        check_command_output(&output, format_args!("cargo {} in {}", self.config.command, anchor_display_dir(cargo_toml)))
    }
}

impl ProductDiscovery for ClippyProcessor {
    fn description(&self) -> &str {
        self.base.description()
    }

    fn processor_type(&self) -> crate::processors::ProcessorType {
        self.base.processor_type()
    }

    fn auto_detect(&self, file_index: &crate::file_index::FileIndex) -> bool {
        crate::processors::ProcessorBase::auto_detect(&self.config.scan, file_index)
    }

    fn config_json(&self) -> Option<String> {
        crate::processors::ProcessorBase::config_json(&self.config)
    }

    fn max_jobs(&self) -> Option<usize> {
        self.config.max_jobs
    }

    fn required_tools(&self) -> Vec<String> {
        vec![self.config.cargo.clone()]
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex, instance_name: &str) -> Result<()> {
        if !crate::processors::scan_root_valid(&self.config.scan) {
            return Ok(());
        }

        discover_directory_products(graph, DirectoryProductOpts {
            scan: &self.config.scan,
            file_index,
            dep_inputs: &self.config.dep_inputs,
            cfg_hash: &self.config,
            siblings: &SiblingFilter {
                extensions: &[".rs", ".toml"],
                excludes: &["/.git/", "/target/", "/.rsconstruct/"],
            },
            processor_name: instance_name,
            output_dir_name: None,
        })
    }

    fn execute(&self, product: &Product) -> Result<()> {
        self.execute_clippy(product.primary_input())
    }
}
