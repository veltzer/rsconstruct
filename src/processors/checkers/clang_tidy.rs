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
        for arg in &self.config.standard.args {
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

impl crate::processors::Processor for ClangTidyProcessor {
    fn scan_config(&self) -> &crate::config::ScanConfig {
        &self.config.standard.scan
    }


    fn description(&self) -> &str {
        "Run clang-tidy static analysis on C/C++ source files"
    }

    fn auto_detect(&self, file_index: &crate::file_index::FileIndex) -> bool {
        crate::processors::checker_auto_detect_with_scan_root(&self.config.standard.scan, file_index)
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
        if !crate::processors::scan_root_valid(&self.config.standard.scan) {
            return Ok(());
        }
        crate::processors::checker_discover(
            graph, &self.config.standard.scan, file_index,
            &self.config.standard.dep_inputs, &self.config.standard.dep_auto,
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
        self.config.standard.max_jobs
    }
}

fn plugin_create(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(ClangTidyProcessor::new(cfg)))
}
inventory::submit! {
    crate::registry::ProcessorPlugin {
        name: "clang_tidy",
        processor_type: crate::processors::ProcessorType::Checker,
        create: plugin_create,
        defconfig_json: crate::registry::default_config_json::<crate::config::ClangTidyConfig>,
        known_fields: crate::registry::typed_known_fields::<crate::config::ClangTidyConfig>,
        output_fields: crate::registry::typed_output_fields::<crate::config::ClangTidyConfig>,
        must_fields: crate::registry::typed_must_fields::<crate::config::ClangTidyConfig>,
        field_descriptions: crate::registry::typed_field_descriptions::<crate::config::ClangTidyConfig>,
    }
}
