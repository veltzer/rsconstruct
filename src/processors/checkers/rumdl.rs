use anyhow::Result;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::RumdlConfig;
use crate::graph::Product;
use crate::processors::{run_command, check_command_output};

pub struct RumdlProcessor {
    project_root: PathBuf,
    config: RumdlConfig,
}

impl RumdlProcessor {
    pub fn new(project_root: PathBuf, config: RumdlConfig) -> Self {
        Self {
            project_root,
            config,
        }
    }

    fn execute_product(&self, product: &Product) -> Result<()> {
        self.lint_files(&[product.inputs[0].as_path()])
    }

    /// Run rumdl on one or more files
    fn lint_files(&self, files: &[&Path]) -> Result<()> {
        let mut cmd = Command::new(&self.config.linter);
        cmd.arg("check");

        for arg in &self.config.args {
            cmd.arg(arg);
        }

        for file in files {
            cmd.arg(file);
        }
        cmd.current_dir(&self.project_root);

        let output = run_command(&mut cmd)?;
        check_command_output(&output, "rumdl")
    }
}

impl_checker!(RumdlProcessor,
    config: config,
    description: "Lint Markdown files using rumdl",
    name: "rumdl",
    execute: execute_product,
    tool_field: linter,
    config_json: true,
    batch: lint_files,
);
