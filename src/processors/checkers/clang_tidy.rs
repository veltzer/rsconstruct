use anyhow::Result;
use std::process::Command;

use crate::config::ClangTidyConfig;
use crate::graph::Product;
use crate::processors::{run_command, check_command_output};

pub struct ClangTidyProcessor {
    config: ClangTidyConfig,
}

impl ClangTidyProcessor {
    pub fn new(config: ClangTidyConfig) -> Self {
        Self { config }
    }

    fn execute_product(&self, product: &Product) -> Result<()> {
        let mut cmd = Command::new("clang-tidy");
        for arg in &self.config.args {
            cmd.arg(arg);
        }
        cmd.arg(product.primary_input());
        // Add -- to separate clang-tidy args from compiler args
        cmd.arg("--");
        for arg in &self.config.compiler_args {
            cmd.arg(arg);
        }

        let output = run_command(&mut cmd)?;
        check_command_output(&output, "clang-tidy")
    }
}

impl crate::processors::ProductDiscovery for ClangTidyProcessor {
    fn scan_config(&self) -> &crate::config::ScanConfig {
        &self.config.scan
    }


    fn description(&self) -> &str {
        "Run clang-tidy static analysis on C/C++ source files"
    }

    fn auto_detect(&self, file_index: &crate::file_index::FileIndex) -> bool {
        crate::processors::checker_auto_detect_with_scan_root(&self.config.scan, file_index)
    }

    fn required_tools(&self) -> Vec<String> {
        vec!["clang-tidy".to_string()]
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

    fn supports_batch(&self) -> bool { false }

    fn execute(&self, product: &Product) -> Result<()> {
        self.execute_product(product)
    }

    fn config_json(&self) -> Option<String> {
        serde_json::to_string(&self.config).ok()
    }

    fn max_jobs(&self) -> Option<usize> {
        self.config.max_jobs
    }
}

inventory::submit! {
    &crate::registry::typed_plugin::<crate::config::ClangTidyConfig>(
        "clang_tidy", |cfg| Box::new(ClangTidyProcessor::new(cfg))
    ) as &dyn crate::registry::RegistryOps
}
