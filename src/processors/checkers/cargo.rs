use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

use crate::config::CargoConfig;
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProductDiscovery, SiblingFilter, discover_directory_products, scan_root_valid, run_command, check_command_output};

pub struct CargoProcessor {
    config: CargoConfig,
}

impl CargoProcessor {
    pub fn new(config: CargoConfig) -> Self {
        Self { config }
    }

    /// Check if cargo processing should be enabled
    fn should_process(&self) -> bool {
        scan_root_valid(&self.config.scan)
    }

    /// Run cargo build in the Cargo.toml's directory
    fn execute_cargo(&self, cargo_toml: &Path) -> Result<()> {
        let project_dir = cargo_toml
            .parent()
            .context("Cargo.toml has no parent directory")?;

        let mut cmd = Command::new(&self.config.cargo);
        cmd.arg(&self.config.command);

        for arg in &self.config.args {
            cmd.arg(arg);
        }

        // Only set current_dir if not empty (root-level Cargo.toml)
        if !project_dir.as_os_str().is_empty() {
            cmd.current_dir(project_dir);
        }

        let output = run_command(&mut cmd)?;
        let display_dir = if project_dir.as_os_str().is_empty() { "." } else { &project_dir.to_string_lossy() };
        check_command_output(&output, format_args!("cargo {} in {}", self.config.command, display_dir))
    }
}

impl ProductDiscovery for CargoProcessor {
    fn description(&self) -> &str {
        "Build Rust projects using Cargo"
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

        discover_directory_products(
            graph,
            &self.config.scan,
            file_index,
            &self.config.extra_inputs,
            &self.config,
            &SiblingFilter {
                extensions: &[".rs", ".toml"], // Match Rust sources and Cargo files
                excludes: &["/.git/", "/target/", "/.rsb/"],
            },
            "cargo",
        )
    }

    fn execute(&self, product: &Product) -> Result<()> {
        self.execute_cargo(&product.inputs[0])
    }

    fn config_json(&self) -> Option<String> {
        serde_json::to_string(&self.config).ok()
    }
}
