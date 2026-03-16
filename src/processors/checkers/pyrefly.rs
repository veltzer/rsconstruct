use anyhow::Result;
use std::path::Path;

use crate::config::PyreflyConfig;
use crate::graph::Product;
use crate::processors::run_checker;

pub struct PyreflyProcessor {
    config: PyreflyConfig,
}

impl PyreflyProcessor {
    pub fn new(config: PyreflyConfig) -> Self {
        Self { config }
    }

    fn execute_product(&self, product: &Product) -> Result<()> {
        self.check_files(&[product.primary_input()])
    }

    /// Run pyrefly on one or more files.
    /// We pass --disable-project-excludes-heuristics because pyrefly's default
    /// project-excludes pattern rejects files in dot-prefixed directories,
    /// and RSConstruct already handles file filtering via its own scan config.
    fn check_files(&self, py_files: &[&Path]) -> Result<()> {
        let mut args = vec!["--disable-project-excludes-heuristics".to_string()];
        args.extend_from_slice(&self.config.args);
        run_checker(&self.config.checker, Some("check"), &args, py_files)
    }
}

impl_checker!(PyreflyProcessor,
    config: config,
    description: "Type-check Python files with pyrefly",
    name: crate::processors::names::PYREFLY,
    execute: execute_product,
    tool_field: checker,
    config_json: true,
    batch: check_files,
);
