use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::{PipConfig, config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProductDiscovery, ProcessorType, clean_outputs, scan_root_valid, run_in_anchor_dir, anchor_display_dir, check_command_output};

pub struct PipProcessor {
    config: PipConfig,
    output_dir: PathBuf,
}

impl PipProcessor {
    pub fn new(config: PipConfig) -> Self {
        Self {
            config,
            output_dir: PathBuf::from("out/pip"),
        }
    }

    fn should_process(&self) -> bool {
        scan_root_valid(&self.config.scan)
    }

    /// Compute the stamp file path for a requirements.txt anchor.
    fn stamp_path(&self, anchor: &Path) -> PathBuf {
        let anchor_dir = anchor.parent().unwrap_or(Path::new(""));
        let name = if anchor_dir.as_os_str().is_empty() {
            "root".to_string()
        } else {
            anchor_dir.display().to_string().replace(['/', '\\'], "_")
        };
        self.output_dir.join(format!("{}.stamp", name))
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
        self.should_process() && !file_index.scan(&self.config.scan, false).is_empty()
    }

    fn required_tools(&self) -> Vec<String> {
        vec![self.config.pip.clone(), "python3".to_string()]
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex) -> Result<()> {
        if !self.should_process() {
            return Ok(());
        }

        let files = file_index.scan(&self.config.scan, false);
        if files.is_empty() {
            return Ok(());
        }

        let hash = Some(config_hash(&self.config));
        let extra = resolve_extra_inputs(&self.config.extra_inputs)?;

        for anchor in files {
            let stamp = self.stamp_path(&anchor);

            let mut inputs: Vec<PathBuf> = Vec::with_capacity(1 + extra.len());
            inputs.push(anchor);
            inputs.extend_from_slice(&extra);

            graph.add_product(inputs, vec![stamp], crate::processors::names::PIP, hash.clone())?;
        }

        Ok(())
    }

    fn execute(&self, product: &Product) -> Result<()> {
        self.execute_pip(product.primary_input())?;

        // Create stamp file
        let stamp = product.outputs.first()
            .context("pip product has no output stamp")?;
        if let Some(parent) = stamp.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create pip output directory: {}", parent.display()))?;
        }
        fs::write(stamp, "")
            .with_context(|| format!("Failed to write pip stamp file: {}", stamp.display()))?;
        Ok(())
    }

    fn clean(&self, product: &Product, verbose: bool) -> Result<usize> {
        clean_outputs(product, crate::processors::names::PIP, verbose)
    }

    fn config_json(&self) -> Option<String> {
        serde_json::to_string(&self.config).ok()
    }
}
