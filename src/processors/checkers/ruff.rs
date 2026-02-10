use anyhow::Result;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::RuffConfig;
use crate::graph::Product;
use crate::processors::{run_command, check_command_output};

pub struct RuffProcessor {
    project_root: PathBuf,
    ruff_config: RuffConfig,
}

impl RuffProcessor {
    pub fn new(project_root: PathBuf, ruff_config: RuffConfig) -> Self {
        Self {
            project_root,
            ruff_config,
        }
    }

    fn execute_product(&self, product: &Product) -> Result<()> {
        self.lint_files(&[product.inputs[0].as_path()])
    }

    /// Run the configured linter on one or more files
    fn lint_files(&self, py_files: &[&Path]) -> Result<()> {
        let linter = &self.ruff_config.linter;
        let mut cmd = Command::new(linter);
        cmd.arg("check");

        for arg in &self.ruff_config.args {
            cmd.arg(arg);
        }

        for file in py_files {
            cmd.arg(file);
        }
        cmd.current_dir(&self.project_root);

        let output = run_command(&mut cmd)?;
        check_command_output(&output, format_args!("{}", linter))
    }
}

impl_checker!(RuffProcessor,
    config: ruff_config,
    description: "Lint Python files with ruff",
    name: "ruff",
    execute: execute_product,
    tool_field: linter,
    config_json: true,
    batch: lint_files,
);
