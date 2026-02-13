use anyhow::Result;
use std::path::Path;

use crate::config::RuffConfig;
use crate::graph::Product;
use crate::processors::run_checker;

pub struct RuffProcessor {
    config: RuffConfig,
}

impl RuffProcessor {
    pub fn new(config: RuffConfig) -> Self {
        Self { config }
    }

    fn execute_product(&self, product: &Product) -> Result<()> {
        self.lint_files(&[product.primary_input()])
    }

    /// Run the configured linter on one or more files
    fn lint_files(&self, py_files: &[&Path]) -> Result<()> {
        run_checker(&self.config.linter, Some("check"), &self.config.args, py_files)
    }
}

impl_checker!(RuffProcessor,
    config: config,
    description: "Lint Python files with ruff",
    name: crate::processors::names::RUFF,
    execute: execute_product,
    tool_field_extra: linter ["python3".to_string()],
    config_json: true,
    batch: lint_files,
);
