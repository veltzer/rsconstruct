use anyhow::{Context, Result};
use std::process::Command;

use crate::config::PandocConfig;
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProcessorBase, ProductDiscovery, run_command, check_command_output};

use super::DiscoverParams;

pub struct PandocProcessor {
    base: ProcessorBase,
    config: PandocConfig,
}

impl PandocProcessor {
    pub fn new(config: PandocConfig) -> Self {
        Self {
            base: ProcessorBase::generator(
                crate::processors::names::PANDOC,
                "Convert documents using pandoc",
            ),
            config,
        }
    }
}

impl ProductDiscovery for PandocProcessor {
    delegate_base!(generator);

    fn required_tools(&self) -> Vec<String> {
        vec![self.config.pandoc.clone()]
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex) -> Result<()> {
        let params = DiscoverParams {
            scan: &self.config.scan,
            extra_inputs: &self.config.extra_inputs,
            config: &self.config,
            output_dir: &self.config.output_dir,
            processor_name: crate::processors::names::PANDOC,
        };
        super::discover_multi_format(graph, file_index, &params, &self.config.formats)
    }

    fn execute(&self, product: &Product) -> Result<()> {
        let input = product.primary_input();
        let output = product.primary_output();

        let format = output.extension()
            .context("pandoc output has no extension")?
            .to_string_lossy();

        crate::processors::ensure_output_dir(output)?;

        let mut cmd = Command::new(&self.config.pandoc);
        // Deterministic output: fixed timestamps
        cmd.env("SOURCE_DATE_EPOCH", "0");
        cmd.arg("--to").arg(format.as_ref());
        // For PDF output, suppress the random trailer ID
        if format.as_ref() == "pdf" {
            cmd.arg("-V").arg(r"header-includes=\pdftrailerid{}");
        }
        for arg in &self.config.args {
            cmd.arg(arg);
        }
        cmd.arg(input);
        cmd.arg("-o").arg(output);

        let out = run_command(&mut cmd)?;
        check_command_output(&out, format_args!("pandoc {}", input.display()))
    }
}
