use anyhow::Result;
use std::path::Path;

use crate::config::ShellcheckConfig;
use crate::graph::Product;
use crate::processors::run_checker;

pub struct ShellcheckProcessor {
    config: ShellcheckConfig,
}

impl ShellcheckProcessor {
    pub fn new(config: ShellcheckConfig) -> Self {
        Self { config }
    }

    fn execute_product(&self, product: &Product) -> Result<()> {
        self.check_files(&[product.primary_input()])
    }

    /// Run shellcheck on one or more files
    fn check_files(&self, files: &[&Path]) -> Result<()> {
        run_checker(&self.config.command, None, &self.config.args, files)
    }
}

impl crate::processors::ProductDiscovery for ShellcheckProcessor {
    fn description(&self) -> &str {
        "Lint shell scripts using shellcheck"
    }

    fn auto_detect(&self, file_index: &crate::file_index::FileIndex) -> bool {
        crate::processors::checker_auto_detect_with_scan_root(&self.config.scan, file_index)
    }

    fn required_tools(&self) -> Vec<String> {
        vec![self.config.command.clone()]
    }

    fn discover(
        &self,
        graph: &mut crate::graph::BuildGraph,
        file_index: &crate::file_index::FileIndex,
        instance_name: &str,
    ) -> anyhow::Result<()> {
        if !crate::processors::scan_root_valid(&self.config.scan) {
            return Ok(());
        }
        crate::processors::checker_discover(
            graph, &self.config.scan, file_index,
            &self.config.dep_inputs, &self.config.dep_auto,
            &self.config, instance_name,
        )
    }

    fn execute(&self, product: &crate::graph::Product) -> anyhow::Result<()> {
        self.execute_product(product)
    }

    fn config_json(&self) -> Option<String> {
        serde_json::to_string(&self.config).ok()
    }

    fn supports_batch(&self) -> bool { self.config.batch }

    fn execute_batch(&self, products: &[&crate::graph::Product]) -> Vec<anyhow::Result<()>> {
        crate::processors::execute_checker_batch(products, |files| self.check_files(files))
    }

    fn max_jobs(&self) -> Option<usize> {
        self.config.max_jobs
    }
}