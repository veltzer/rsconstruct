use anyhow::{Context, Result};
use std::fs;
use std::process::Command;

use crate::config::ChromiumConfig;
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProcessorBase, ProductDiscovery, run_command, check_command_output};

use super::DiscoverParams;

pub struct ChromiumProcessor {
    base: ProcessorBase,
    config: ChromiumConfig,
}

impl ChromiumProcessor {
    pub fn new(config: ChromiumConfig) -> Self {
        Self {
            base: ProcessorBase::generator(
                crate::processors::names::CHROMIUM,
                "Convert HTML to PDF using headless Chromium",
            ),
            config,
        }
    }
}

impl ProductDiscovery for ChromiumProcessor {
    delegate_base!(generator);

    fn required_tools(&self) -> Vec<String> {
        vec![self.config.command.clone()]
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex, instance_name: &str) -> Result<()> {
        let params = DiscoverParams {
            scan: &self.config.scan,
            dep_inputs: &self.config.dep_inputs,
            config: &self.config,
            output_dir: &self.config.output_dir,
            processor_name: instance_name,
        };
        super::discover_single_format(graph, file_index, &params, "pdf")
    }

    fn execute(&self, product: &Product) -> Result<()> {
        let input = product.primary_input();
        let output = product.primary_output();

        crate::processors::ensure_output_dir(output)?;

        // Convert the input path to an absolute file:// URL for Chromium
        let abs_input = fs::canonicalize(input)
            .with_context(|| format!("Failed to resolve absolute path for: {}", input.display()))?;
        let input_url = format!("file://{}", abs_input.display());

        let mut cmd = Command::new(&self.config.command);
        cmd.arg("--headless");
        cmd.arg("--disable-gpu");
        cmd.arg("--no-sandbox");
        cmd.arg(format!("--print-to-pdf={}", output.display()));
        for arg in &self.config.args {
            cmd.arg(arg);
        }
        cmd.arg(&input_url);

        let out = run_command(&mut cmd)?;
        check_command_output(&out, format_args!("chromium {}", input.display()))
    }
}
