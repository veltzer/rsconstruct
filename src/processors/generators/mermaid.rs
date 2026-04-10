use anyhow::Result;
use std::process::Command;

use crate::config::MermaidConfig;
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProcessorBase, ProductDiscovery, run_command, check_command_output};

use super::DiscoverParams;

pub struct MermaidProcessor {
    base: ProcessorBase,
    config: MermaidConfig,
}

impl MermaidProcessor {
    pub fn new(config: MermaidConfig) -> Self {
        Self {
            base: ProcessorBase::generator(
                crate::processors::names::MERMAID,
                "Convert Mermaid diagrams to PNG/SVG/PDF",
            ),
            config,
        }
    }
}

impl ProductDiscovery for MermaidProcessor {
    delegate_base!(generator);

    fn required_tools(&self) -> Vec<String> {
        vec![self.config.command.clone(), "node".to_string()]
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex, instance_name: &str) -> Result<()> {
        let params = DiscoverParams {
            scan: &self.config.scan,
            dep_inputs: &self.config.dep_inputs,
            config: &self.config,
            output_dir: &self.config.output_dir,
            processor_name: instance_name,
        };
        super::discover_multi_format(graph, file_index, &params, &self.config.formats)
    }

    fn execute(&self, product: &Product) -> Result<()> {
        let input = product.primary_input();
        let output = product.primary_output();

        crate::processors::ensure_output_dir(output)?;

        let mut cmd = Command::new(&self.config.command);
        cmd.arg("-i").arg(input);
        cmd.arg("-o").arg(output);
        for arg in &self.config.args {
            cmd.arg(arg);
        }

        let out = run_command(&mut cmd)?;
        check_command_output(&out, format_args!("mmdc {}", input.display()))
    }
}
