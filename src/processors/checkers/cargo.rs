use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::{CargoConfig, config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProductDiscovery, scan_root, run_command, check_command_output};

pub struct CargoProcessor {
    config: CargoConfig,
}

impl CargoProcessor {
    pub fn new(_project_root: PathBuf, config: CargoConfig) -> Self {
        Self { config }
    }

    /// Check if cargo processing should be enabled
    fn should_process(&self) -> bool {
        scan_root(&self.config.scan).as_os_str().is_empty() || scan_root(&self.config.scan).exists()
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

        let cargo_tomls = file_index.scan(&self.config.scan, true);
        if cargo_tomls.is_empty() {
            return Ok(());
        }

        let cfg_hash = Some(config_hash(&self.config));
        let extra = resolve_extra_inputs(&self.config.extra_inputs)?;

        for cargo_toml in cargo_tomls {
            let project_dir = cargo_toml
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_default();

            // Collect all Rust source files under the project directory as inputs
            // so that changes to any .rs file trigger a rebuild.
            let rust_files = file_index.query(
                &project_dir,
                &[".rs", ".toml"], // Match Rust sources and Cargo files
                &["/.git/", "/target/", "/.rsb/"],
                &[],
                &[],
            );

            let mut inputs: Vec<PathBuf> = Vec::new();
            // Cargo.toml first so product display shows it
            inputs.push(cargo_toml.clone());
            for file in &rust_files {
                if *file != cargo_toml {
                    inputs.push(file.clone());
                }
            }
            inputs.extend(extra.clone());

            // Empty outputs: cache entry = success record
            // (Cargo manages its own target/ directory)
            graph.add_product(inputs, vec![], "cargo", cfg_hash.clone())?;
        }

        Ok(())
    }

    fn execute(&self, product: &Product) -> Result<()> {
        self.execute_cargo(&product.inputs[0])
    }

    fn config_json(&self) -> Option<String> {
        serde_json::to_string(&self.config).ok()
    }
}
