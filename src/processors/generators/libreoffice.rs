use anyhow::{Context, Result};
use std::fs;
use std::process::Command;

use crate::config::LibreofficeConfig;
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProcessorBase, ProductDiscovery, run_command, check_command_output};

use super::DiscoverParams;

pub struct LibreofficeProcessor {
    base: ProcessorBase,
    config: LibreofficeConfig,
}

impl LibreofficeProcessor {
    pub fn new(config: LibreofficeConfig) -> Self {
        Self {
            base: ProcessorBase::generator(
                crate::processors::names::LIBREOFFICE,
                "Convert LibreOffice documents to PDF/PPTX",
            ),
            config,
        }
    }
}

impl ProductDiscovery for LibreofficeProcessor {
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
        vec![self.config.command.clone(), "flock".into()]
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
            .context("libreoffice output has no extension")?
            .to_string_lossy();

        let output_dir = output.parent()
            .context("libreoffice output has no parent directory")?;

        fs::create_dir_all(output_dir)
            .with_context(|| format!("Failed to create libreoffice output directory: {}", output_dir.display()))?;

        // Use flock to serialize LibreOffice invocations (it can't run multiple instances)
        let mut cmd = Command::new("flock");
        cmd.arg("/tmp/rsconstruct_libreoffice");
        cmd.arg(&self.config.command);
        cmd.arg("--headless");
        cmd.arg("--convert-to").arg(format.as_ref());
        cmd.arg("--outdir").arg(output_dir);
        for arg in &self.config.args {
            cmd.arg(arg);
        }
        cmd.arg(input);

        let out = run_command(&mut cmd)?;
        check_command_output(&out, format_args!("libreoffice {}", input.display()))
    }
}
