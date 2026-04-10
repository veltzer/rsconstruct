use anyhow::{Context, Result};
use std::fs;
use std::process::Command;

use crate::config::A2xConfig;
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProcessorBase, ProductDiscovery, run_command, check_command_output};

use super::DiscoverParams;

pub struct A2xProcessor {
    base: ProcessorBase,
    config: A2xConfig,
}

impl A2xProcessor {
    pub fn new(config: A2xConfig) -> Self {
        Self {
            base: ProcessorBase::generator(
                crate::processors::names::A2X,
                "Convert AsciiDoc to PDF using a2x",
            ),
            config,
        }
    }
}

impl ProductDiscovery for A2xProcessor {
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
        vec![self.config.a2x.clone(), "python3".to_string()]
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

        let mut cmd = Command::new(&self.config.a2x);
        for arg in &self.config.args {
            cmd.arg(arg);
        }
        cmd.arg(input);

        let out = run_command(&mut cmd)?;
        check_command_output(&out, format_args!("a2x {}", input.display()))?;

        // a2x generates the PDF next to the input file — move it to the output path
        let stem = input.file_stem()
            .context("a2x input has no file stem")?;
        let generated = input.with_file_name(format!("{}.pdf", stem.to_string_lossy()));

        if generated != *output && generated.exists() {
            fs::rename(&generated, output)
                .with_context(|| format!("Failed to move a2x output from {} to {}", generated.display(), output.display()))?;
        }

        Ok(())
    }
}
