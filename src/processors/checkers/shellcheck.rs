use anyhow::Result;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::ShellcheckConfig;
use crate::graph::Product;
use crate::processors::{scan_root_valid, run_command, check_command_output};

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

    fn execute_product(&self, product: &Product) -> Result<()> {
        self.check_files(&[product.inputs[0].as_path()])
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

impl_checker!(ShellcheckProcessor,
    config: config,
    description: "Lint shell scripts using shellcheck",
    name: "shellcheck",
    execute: execute_product,
    guard: should_lint,
    tool_field: checker,
    config_json: true,
    batch: check_files,
);
