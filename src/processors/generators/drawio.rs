use anyhow::{Context, Result};
use std::process::Command;

use crate::config::DrawioConfig;
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProcessorBase, ProductDiscovery, run_command, check_command_output};

use super::DiscoverParams;

pub struct DrawioProcessor {
    base: ProcessorBase,
    config: DrawioConfig,
}

impl DrawioProcessor {
    pub fn new(config: DrawioConfig) -> Self {
        Self {
            base: ProcessorBase::generator(
                crate::processors::names::DRAWIO,
                "Convert Draw.io diagrams to PNG/SVG/PDF",
            ),
            config,
        }
    }
}

impl ProductDiscovery for DrawioProcessor {
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
        super::discover_multi_format(graph, file_index, &params, &self.config.formats)
    }

    fn execute(&self, product: &Product) -> Result<()> {
        let input = product.primary_input();
        let output = product.primary_output();

        let format = output.extension()
            .context("drawio output has no extension")?
            .to_string_lossy();

        crate::processors::ensure_output_dir(output)?;

        let mut cmd = Command::new(&self.config.command);
        cmd.arg("--export");
        cmd.arg("--format").arg(format.as_ref());
        cmd.arg("--output").arg(output);
        for arg in &self.config.args {
            cmd.arg(arg);
        }
        cmd.arg(input);

        let out = run_command(&mut cmd)?;
        check_command_output(&out, format_args!("drawio {}", input.display()))
    }
}
