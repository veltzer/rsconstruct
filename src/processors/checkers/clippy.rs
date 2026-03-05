use anyhow::Result;
use std::path::Path;
use std::process::Command;

use crate::config::ClippyConfig;
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProductDiscovery, SiblingFilter, DirectoryProductOpts, discover_directory_products, scan_root_valid, run_in_anchor_dir, anchor_display_dir, check_command_output};

pub struct ClippyProcessor {
    config: ClippyConfig,
}

impl ClippyProcessor {
    pub fn new(config: ClippyConfig) -> Self {
        Self { config }
    }

    /// Check if clippy processing should be enabled
    fn should_process(&self) -> bool {
        scan_root_valid(&self.config.scan)
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
        "Lint Rust projects using Cargo Clippy"
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        self.should_process() && !file_index.scan(&self.config.scan, true).is_empty()
    }

    fn required_tools(&self) -> Vec<String> {
        vec![self.config.cargo.clone()]
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
                extensions: &[".rs", ".toml"],
                excludes: &["/.git/", "/target/", "/.rsbuild/"],
            },
            processor_name: crate::processors::names::CLIPPY,
            output_dir_name: None,
        })
    }

    fn execute(&self, product: &Product) -> Result<()> {
        self.execute_clippy(product.primary_input())
    }

    fn config_json(&self) -> Option<String> {
        serde_json::to_string(&self.config).ok()
    }
}
