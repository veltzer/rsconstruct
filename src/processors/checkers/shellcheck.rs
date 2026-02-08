use anyhow::Result;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::ShellcheckConfig;
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProductDiscovery, discover_checker_products, scan_root_valid, run_command, check_command_output, execute_checker_batch};

pub struct ShellcheckProcessor {
    project_root: PathBuf,
    config: ShellcheckConfig,
}

impl ShellcheckProcessor {
    pub fn new(project_root: PathBuf, config: ShellcheckConfig) -> Self {
        Self {
            project_root,
            config,
        }
    }

    /// Check if shell linting should be enabled
    fn should_lint(&self) -> bool {
        scan_root_valid(&self.config.scan)
    }

    /// Run shellcheck on one or more files
    fn check_files(&self, files: &[&Path]) -> Result<()> {
        let mut cmd = Command::new(&self.config.checker);

        for arg in &self.config.args {
            cmd.arg(arg);
        }

        for file in files {
            cmd.arg(file);
        }
        cmd.current_dir(&self.project_root);

        let output = run_command(&mut cmd)?;
        check_command_output(&output, "shellcheck")
    }
}

impl ProductDiscovery for ShellcheckProcessor {
    fn description(&self) -> &str {
        "Lint shell scripts using shellcheck"
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        self.should_lint() && !file_index.scan(&self.config.scan, true).is_empty()
    }

    fn required_tools(&self) -> Vec<String> {
        vec![self.config.checker.clone()]
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex) -> Result<()> {
        if !self.should_lint() {
            return Ok(());
        }
        discover_checker_products(
            graph,
            &self.config.scan,
            file_index,
            &self.config.extra_inputs,
            &self.config,
            "shellcheck",
        )
    }

    fn execute(&self, product: &Product) -> Result<()> {
        self.check_files(&[product.inputs[0].as_path()])
    }

    fn supports_batch(&self) -> bool {
        true
    }

    fn execute_batch(&self, products: &[&Product]) -> Vec<Result<()>> {
        execute_checker_batch(
            products,
            |files| self.check_files(files),
        )
    }

    fn config_json(&self) -> Option<String> {
        serde_json::to_string(&self.config).ok()
    }
}
