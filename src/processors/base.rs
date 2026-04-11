use serde::Serialize;
use crate::config::StandardConfig;
use crate::graph::Product;
use crate::processors::ProcessorType;

/// Common base for all processors. Holds fields needed by boilerplate
/// Processor methods so each processor doesn't repeat them.
#[allow(dead_code)]
pub struct ProcessorBase {
    /// Processor name constant (e.g., "marp", "pylint")
    pub name: &'static str,
    /// Human-readable description
    pub description: &'static str,
    /// Generator or Checker
    pub processor_type: ProcessorType,
}

#[allow(dead_code)]
impl ProcessorBase {
    pub fn generator(name: &'static str, description: &'static str) -> Self {
        Self { name, description, processor_type: ProcessorType::Generator }
    }

    pub fn creator(name: &'static str, description: &'static str) -> Self {
        Self { name, description, processor_type: ProcessorType::Creator }
    }

    pub fn checker(name: &'static str, description: &'static str) -> Self {
        Self { name, description, processor_type: ProcessorType::Checker }
    }

    pub fn explicit(name: &'static str, description: &'static str) -> Self {
        Self { name, description, processor_type: ProcessorType::Explicit }
    }

    pub fn description(&self) -> &str {
        self.description
    }

    pub fn processor_type(&self) -> ProcessorType {
        self.processor_type
    }

    pub fn config_json<C: Serialize>(config: &C) -> Option<String> {
        serde_json::to_string(config).ok()
    }

    pub fn clean(product: &Product, name: &str, verbose: bool) -> anyhow::Result<usize> {
        crate::processors::clean_outputs(product, name, verbose)
    }

    pub fn clean_output_dir(product: &Product, name: &str, verbose: bool) -> anyhow::Result<usize> {
        crate::processors::clean_output_dir(product, name, verbose)
    }

    pub fn auto_detect(scan: &StandardConfig, file_index: &crate::file_index::FileIndex) -> bool {
        crate::processors::scan_root_valid(scan) && !file_index.scan(scan, true).is_empty()
    }
}
