use anyhow::Result;
use std::process::Command;

use crate::config::SassConfig;
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProcessorBase, ProductDiscovery, run_command, check_command_output};

use super::DiscoverParams;

pub struct SassProcessor {
    base: ProcessorBase,
    config: SassConfig,
}

impl SassProcessor {
    pub fn new(config: SassConfig) -> Self {
        Self {
            base: ProcessorBase::generator(
                crate::processors::names::SASS,
                "Compile SCSS/SASS files to CSS",
            ),
            config,
        }
    }
}

impl ProductDiscovery for SassProcessor {
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
        super::discover_single_format(graph, file_index, &params, "css")
    }

    fn execute(&self, product: &Product) -> Result<()> {
        let input = product.primary_input();
        let output = product.primary_output();

        crate::processors::ensure_output_dir(output)?;

        let mut cmd = Command::new(&self.config.command);
        for arg in &self.config.args {
            cmd.arg(arg);
        }
        cmd.arg(input).arg(output);

        let out = run_command(&mut cmd)?;
        check_command_output(&out, format_args!("sass {}", input.display()))
    }
}
