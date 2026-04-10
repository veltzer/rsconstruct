use anyhow::{Context, Result};
use std::fs;
use std::process::Command;

use crate::config::ObjdumpConfig;
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProcessorBase, ProductDiscovery, run_command_capture, check_command_output};

use super::DiscoverParams;

pub struct ObjdumpProcessor {
    base: ProcessorBase,
    config: ObjdumpConfig,
}

impl ObjdumpProcessor {
    pub fn new(config: ObjdumpConfig) -> Self {
        Self {
            base: ProcessorBase::generator(
                crate::processors::names::OBJDUMP,
                "Disassemble binaries using objdump",
            ),
            config,
        }
    }
}

impl ProductDiscovery for ObjdumpProcessor {
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

    fn required_tools(&self) -> Vec<String> {
        vec!["objdump".to_string()]
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex, instance_name: &str) -> Result<()> {
        let params = DiscoverParams {
            scan: &self.config.scan,
            dep_inputs: &self.config.dep_inputs,
            config: &self.config,
            output_dir: &self.config.output_dir,
            processor_name: instance_name,
        };
        super::discover_single_format(graph, file_index, &params, "dis")
    }

    fn execute(&self, product: &Product) -> Result<()> {
        let input = product.primary_input();
        let output = product.primary_output();

        crate::processors::ensure_output_dir(output)?;

        let mut cmd = Command::new("objdump");
        cmd.arg("--disassemble").arg("--source");
        for arg in &self.config.args {
            cmd.arg(arg);
        }
        cmd.arg(input);

        let out = run_command_capture(&mut cmd)?;
        check_command_output(&out, format_args!("objdump {}", input.display()))?;

        fs::write(output, &out.stdout)
            .with_context(|| format!("Failed to write objdump output: {}", output.display()))?;

        Ok(())
    }
}
