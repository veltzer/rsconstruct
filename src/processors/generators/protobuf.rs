use anyhow::Result;
use std::process::Command;

use crate::config::ProtobufConfig;
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProcessorBase, ProductDiscovery, run_command, check_command_output};

use super::DiscoverParams;

pub struct ProtobufProcessor {
    base: ProcessorBase,
    config: ProtobufConfig,
}

impl ProtobufProcessor {
    pub fn new(config: ProtobufConfig) -> Self {
        Self {
            base: ProcessorBase::generator(
                crate::processors::names::PROTOBUF,
                "Compile Protocol Buffer files",
            ),
            config,
        }
    }
}

impl ProductDiscovery for ProtobufProcessor {
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
        super::discover_single_format(graph, file_index, &params, "pb.cc")
    }

    fn execute(&self, product: &Product) -> Result<()> {
        let input = product.primary_input();
        let output = product.primary_output();
        let output_dir = output.parent().unwrap_or(std::path::Path::new("."));

        crate::processors::ensure_output_dir(output)?;

        let mut cmd = Command::new(&self.config.command);
        // Set the proto path to the directory containing the input file
        if let Some(parent) = input.parent() {
            cmd.arg(format!("--proto_path={}", parent.display()));
        }
        cmd.arg(format!("--cpp_out={}", output_dir.display()));
        for arg in &self.config.args {
            cmd.arg(arg);
        }
        cmd.arg(input);

        let out = run_command(&mut cmd)?;
        check_command_output(&out, format_args!("protoc {}", input.display()))
    }
}
