use anyhow::Result;
use std::path::Path;

use crate::config::EslintConfig;
use crate::graph::Product;
use crate::processors::run_checker;

pub struct EslintProcessor {
    config: EslintConfig,
}

impl EslintProcessor {
    pub fn new(config: EslintConfig) -> Self {
        Self { config }
    }

    fn execute_product(&self, product: &Product) -> Result<()> {
        self.lint_files(&[product.primary_input()])
    }

    fn lint_files(&self, files: &[&Path]) -> Result<()> {
        run_checker(&self.config.linter, None, &self.config.args, files)
    }
}

impl_checker!(EslintProcessor,
    config: config,
    description: "Lint JavaScript/TypeScript files with eslint",
    name: crate::processors::names::ESLINT,
    execute: execute_product,
    tool_field_extra: linter ["node".to_string()],
    config_json: true,
    batch: lint_files,
);
