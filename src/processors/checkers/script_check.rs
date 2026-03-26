use anyhow::Result;
use std::path::Path;

use crate::config::ScriptCheckConfig;
use crate::graph::Product;
use crate::processors::{scan_root_valid, run_checker};

pub struct ScriptCheckProcessor {
    config: ScriptCheckConfig,
}

impl ScriptCheckProcessor {
    pub fn new(config: ScriptCheckConfig) -> Self {
        Self { config }
    }

    fn should_process(&self) -> bool {
        scan_root_valid(&self.config.scan) && !self.config.linter.is_empty()
    }

    fn execute_product(&self, product: &Product) -> Result<()> {
        self.check_files(&[product.primary_input()])
    }

    fn check_files(&self, files: &[&Path]) -> Result<()> {
        run_checker(&self.config.linter, None, &self.config.args, files)
    }
}

impl_checker!(ScriptCheckProcessor,
    config: config,
    description: "Run a user-configured script as a checker",
    name: crate::processors::names::SCRIPT_CHECK,
    execute: execute_product,
    guard: should_process,
    tool_field: linter,
    config_json: true,
    batch: check_files,
);
