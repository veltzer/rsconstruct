use anyhow::Result;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::{PipConfig, output_config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProductDiscovery, ProcessorType, scan_root_valid, run_in_anchor_dir, anchor_display_dir, check_command_output};

pub struct PipProcessor {
    config: PipConfig,
}

impl PipProcessor {
    pub fn new(config: PipConfig) -> Self {
        Self { config }
    }

    /// Run pip install -r requirements.txt in the file's directory
    fn execute_pip(&self, requirements_txt: &Path) -> Result<()> {
        let mut cmd = Command::new(&self.config.pip);
        cmd.arg("install");
        cmd.arg("-r").arg(requirements_txt.file_name().unwrap_or_default());
        for arg in &self.config.args {
            cmd.arg(arg);
        }
        let output = run_in_anchor_dir(&mut cmd, requirements_txt)?;
        check_command_output(&output, format_args!("pip install in {}", anchor_display_dir(requirements_txt)))
    }
}

impl ProductDiscovery for PipProcessor {
    fn description(&self) -> &str {
        "Install Python dependencies using pip"
    }

    fn processor_type(&self) -> ProcessorType {
        ProcessorType::MassGenerator
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        scan_root_valid(&self.config.scan) && !file_index.scan(&self.config.scan, false).is_empty()
    }

    fn required_tools(&self) -> Vec<String> {
        vec![self.config.pip.clone(), "python3".to_string()]
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex) -> Result<()> {
        if !scan_root_valid(&self.config.scan) {
            return Ok(());
        }

        let files = file_index.scan(&self.config.scan, false);
        if files.is_empty() {
            return Ok(());
        }

        let hash = Some(output_config_hash(&self.config, &[]));
        let extra = resolve_extra_inputs(&self.config.extra_inputs)?;

        for anchor in files {
            let mut inputs: Vec<PathBuf> = Vec::with_capacity(1 + extra.len());
            inputs.push(anchor.clone());
            inputs.extend_from_slice(&extra);

            graph.add_product(inputs, vec![], crate::processors::names::PIP, hash.clone())?;
        }

        Ok(())
    }

    fn execute(&self, product: &Product) -> Result<()> {
        self.execute_pip(product.primary_input())
    }

    fn clean(&self, _product: &Product, _verbose: bool) -> Result<usize> {
        Ok(0)
    }

    fn config_json(&self) -> Option<String> {
        serde_json::to_string(&self.config).ok()
    }

    fn max_jobs(&self) -> Option<usize> {
        self.config.max_jobs
    }
}
