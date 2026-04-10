use anyhow::{Context, Result};
use std::fs;
use std::process::Command;

use crate::config::Markdown2htmlConfig;
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProcessorBase, ProductDiscovery, run_command_capture, check_command_output};

use super::DiscoverParams;

pub struct Markdown2htmlProcessor {
    base: ProcessorBase,
    config: Markdown2htmlConfig,
}

impl Markdown2htmlProcessor {
    pub fn new(config: Markdown2htmlConfig) -> Self {
        Self {
            base: ProcessorBase::generator(
                crate::processors::names::MARKDOWN2HTML,
                "Convert Markdown to HTML using markdown",
            ),
            config,
        }
    }
}

impl ProductDiscovery for Markdown2htmlProcessor {
    delegate_base!(generator);

    fn required_tools(&self) -> Vec<String> {
        vec![self.config.command.clone(), "perl".to_string()]
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex, instance_name: &str) -> Result<()> {
        let params = DiscoverParams {
            scan: &self.config.scan,
            dep_inputs: &self.config.dep_inputs,
            config: &self.config,
            output_dir: &self.config.output_dir,
            processor_name: instance_name,
        };
        super::discover_single_format(graph, file_index, &params, "html")
    }

    fn execute(&self, product: &Product) -> Result<()> {
        let input = product.primary_input();
        let output = product.primary_output();

        crate::processors::ensure_output_dir(output)?;

        let mut cmd = Command::new(&self.config.command);
        for arg in &self.config.args {
            cmd.arg(arg);
        }
        cmd.arg(input);

        let out = run_command_capture(&mut cmd)?;
        check_command_output(&out, format_args!("markdown {}", input.display()))?;

        fs::write(output, &out.stdout)
            .with_context(|| format!("Failed to write markdown output: {}", output.display()))?;

        Ok(())
    }
}
