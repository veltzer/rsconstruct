use anyhow::Result;

use crate::config::CppcheckConfig;
use crate::graph::Product;
use crate::processors::{scan_root_valid, run_checker};

pub struct CppcheckProcessor {
    config: CppcheckConfig,
}

impl CppcheckProcessor {
    pub fn new(config: CppcheckConfig) -> Self {
        Self { config }
    }

    fn should_process(&self) -> bool {
        scan_root_valid(&self.config.scan)
    }

    fn execute_product(&self, product: &Product) -> Result<()> {
        run_checker("cppcheck", None, &self.config.args, &[product.primary_input()])
    }
}

impl_checker!(CppcheckProcessor,
    config: config,
    description: "Run cppcheck static analysis on C/C++ source files",
    name: crate::processors::names::CPPCHECK,
    execute: execute_product,
    guard: should_process,
    tools: ["cppcheck".to_string()],
    config_json: true,
);
