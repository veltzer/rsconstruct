use anyhow::{Context, Result};

use crate::config::Yaml2jsonConfig;
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProcessorBase, ProductDiscovery};

use super::DiscoverParams;

pub struct Yaml2jsonProcessor {
    base: ProcessorBase,
    config: Yaml2jsonConfig,
}

impl Yaml2jsonProcessor {
    pub fn new(config: Yaml2jsonConfig) -> Self {
        Self {
            base: ProcessorBase::generator(
                crate::processors::names::YAML2JSON,
                "Convert YAML to JSON (in-process)",
            ),
            config,
        }
    }
}

impl ProductDiscovery for Yaml2jsonProcessor {
    fn description(&self) -> &str {
        self.base.description()
    }

    fn processor_type(&self) -> crate::processors::ProcessorType {
        self.base.processor_type()
    }

    fn auto_detect(&self, file_index: &crate::file_index::FileIndex) -> bool {
        crate::processors::ProcessorBase::auto_detect(&self.config.scan, file_index)
    }

    fn config_json(&self) -> Option<String> {
        crate::processors::ProcessorBase::config_json(&self.config)
    }

    fn max_jobs(&self) -> Option<usize> {
        self.config.max_jobs
    }

    fn clean(&self, product: &crate::graph::Product, verbose: bool) -> anyhow::Result<usize> {
        crate::processors::ProcessorBase::clean(product, &product.processor, verbose)
    }

    fn is_native(&self) -> bool { true }

    fn required_tools(&self) -> Vec<String> {
        Vec::new()
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex, instance_name: &str) -> Result<()> {
        let params = DiscoverParams {
            scan: &self.config.scan,
            dep_inputs: &self.config.dep_inputs,
            config: &self.config,
            output_dir: &self.config.output_dir,
            processor_name: instance_name,
        };
        super::discover_single_format(graph, file_index, &params, "json")
    }

    fn execute(&self, product: &Product) -> Result<()> {
        let input = product.primary_input();
        let output = product.primary_output();

        crate::processors::ensure_output_dir(output)?;

        let contents = std::fs::read_to_string(input)
            .with_context(|| format!("Failed to read {}", input.display()))?;

        let value: serde_json::Value = serde_yml::from_str(&contents)
            .with_context(|| format!("Failed to parse YAML from {}", input.display()))?;

        let json = serde_json::to_string_pretty(&value)
            .with_context(|| format!("Failed to serialize JSON for {}", input.display()))?;

        std::fs::write(output, json)
            .with_context(|| format!("Failed to write {}", output.display()))?;

        Ok(())
    }

    fn supports_batch(&self) -> bool {
        self.config.batch
    }

    fn execute_batch(&self, products: &[&Product]) -> Vec<Result<()>> {
        products.iter().map(|p| self.execute(p)).collect()
    }
}
