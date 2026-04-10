use anyhow::Result;

use crate::config::CheckpatchConfig;
use crate::graph::Product;
use crate::processors::run_checker;

pub struct CheckpatchProcessor {
    config: CheckpatchConfig,
}

impl CheckpatchProcessor {
    pub fn new(config: CheckpatchConfig) -> Self {
        Self { config }
    }

    fn execute_product(&self, product: &Product) -> Result<()> {
        let mut args = vec![
            "--no-tree".to_string(),
            "-f".to_string(),
        ];
        args.extend(self.config.args.iter().cloned());
        run_checker("checkpatch.pl", None, &args, &[product.primary_input()])
    }
}

impl crate::processors::ProductDiscovery for CheckpatchProcessor {
    fn description(&self) -> &str {
        "Run kernel checkpatch.pl on C source files"
    }

    fn auto_detect(&self, file_index: &crate::file_index::FileIndex) -> bool {
        crate::processors::checker_auto_detect_with_scan_root(&self.config.scan, file_index)
    }

    fn required_tools(&self) -> Vec<String> {
        vec!["checkpatch.pl".to_string(), "perl".to_string()]
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

    fn max_jobs(&self) -> Option<usize> {
        self.config.max_jobs
    }
}