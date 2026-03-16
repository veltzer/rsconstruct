use anyhow::{Context, Result};
use std::fs;
use std::process::Command;

use crate::config::LibreofficeConfig;
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProductDiscovery, ProcessorType, clean_outputs, scan_root_valid, run_command, check_command_output};

use super::DiscoverParams;

pub struct LibreofficeProcessor {
    config: LibreofficeConfig,
}

impl LibreofficeProcessor {
    pub fn new(config: LibreofficeConfig) -> Self {
        Self { config }
    }
}

impl ProductDiscovery for LibreofficeProcessor {
    fn description(&self) -> &str {
        "Convert LibreOffice documents to PDF/PPTX"
    }

    fn processor_type(&self) -> ProcessorType {
        ProcessorType::Generator
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        scan_root_valid(&self.config.scan) && !file_index.scan(&self.config.scan, true).is_empty()
    }

    fn required_tools(&self) -> Vec<String> {
        vec![self.config.libreoffice_bin.clone(), "flock".into()]
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex) -> Result<()> {
        let params = DiscoverParams {
            scan: &self.config.scan,
            extra_inputs: &self.config.extra_inputs,
            config: &self.config,
            output_dir: &self.config.output_dir,
            processor_name: crate::processors::names::LIBREOFFICE,
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
        cmd.arg(&self.config.libreoffice_bin);
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

    fn clean(&self, product: &Product, verbose: bool) -> Result<usize> {
        clean_outputs(product, crate::processors::names::LIBREOFFICE, verbose)
    }

    fn config_json(&self) -> Option<String> {
        serde_json::to_string(&self.config).ok()
    }
}
