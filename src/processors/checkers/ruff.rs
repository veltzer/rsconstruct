use anyhow::Result;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::RuffConfig;
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProductDiscovery, discover_checker_products, run_command, check_command_output, execute_checker_batch};

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

impl ProductDiscovery for RuffProcessor {
    fn description(&self) -> &str {
        "Lint Python files with ruff"
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        !file_index.scan(&self.ruff_config.scan, true).is_empty()
    }

    fn required_tools(&self) -> Vec<String> {
        vec![self.ruff_config.linter.clone()]
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex) -> Result<()> {
        discover_checker_products(
            graph,
            &self.ruff_config.scan,
            file_index,
            &self.ruff_config.extra_inputs,
            &self.ruff_config,
            "ruff",
        )
    }

    fn execute(&self, product: &Product) -> Result<()> {
        self.lint_files(&[product.inputs[0].as_path()])
    }

    fn supports_batch(&self) -> bool {
        true
    }

    fn execute_batch(&self, products: &[&Product]) -> Vec<Result<()>> {
        execute_checker_batch(
            products,
            |files| self.lint_files(files),
        )
    }

    fn config_json(&self) -> Option<String> {
        serde_json::to_string(&self.ruff_config).ok()
    }
}
