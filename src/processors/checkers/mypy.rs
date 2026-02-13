use anyhow::Result;
use std::path::Path;

use crate::config::MypyConfig;
use crate::graph::Product;
use crate::processors::{run_checker, config_file_inputs};

pub struct MypyProcessor {
    config: MypyConfig,
}

impl MypyProcessor {
    pub fn new(config: MypyConfig) -> Self {
        Self { config }
    }

    fn execute_product(&self, product: &Product) -> Result<()> {
        self.check_files(&[product.primary_input()])
    }

    /// Return extra inputs for discover: mypy.ini if it exists
    fn mypy_ini_inputs(&self) -> Vec<String> {
        config_file_inputs("mypy.ini")
    }

    /// Run mypy on one or more files
    fn check_files(&self, py_files: &[&Path]) -> Result<()> {
        run_checker(&self.config.checker, None, &self.config.args, py_files)
    }
}

impl_checker!(MypyProcessor,
    config: config,
    description: "Type-check Python files with mypy",
    name: crate::processors::names::MYPY,
    execute: execute_product,
    tool_field: checker,
    config_json: true,
    batch: check_files,
    extra_discover_inputs: mypy_ini_inputs,
);
