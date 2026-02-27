use anyhow::Result;

use crate::config::JsonlintConfig;
use crate::graph::Product;
use crate::processors::run_checker;

pub struct JsonlintProcessor {
    config: JsonlintConfig,
}

impl JsonlintProcessor {
    pub fn new(config: JsonlintConfig) -> Self {
        Self { config }
    }

    fn execute_product(&self, product: &Product) -> Result<()> {
        run_checker(&self.config.linter, None, &self.config.args, &[product.primary_input()])
    }
}

impl_checker!(JsonlintProcessor,
    config: config,
    description: "Lint JSON files with jsonlint",
    name: crate::processors::names::JSONLINT,
    execute: execute_product,
    tool_field_extra: linter ["python3".to_string()],
    config_json: true,
);
