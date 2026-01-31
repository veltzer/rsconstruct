use anyhow::{Context, Result};
use std::path::PathBuf;
use std::process::Command;

use crate::config::{MakeConfig, config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use super::{ProductDiscovery, scan_root, validate_stub_product, ensure_stub_dir, write_stub, clean_outputs, log_command};

const MAKE_STUB_DIR: &str = "out/make";

pub struct MakeProcessor {
    project_root: PathBuf,
    config: MakeConfig,
    stub_dir: PathBuf,
}

impl MakeProcessor {
    pub fn new(project_root: PathBuf, config: MakeConfig) -> Self {
        let stub_dir = project_root.join(MAKE_STUB_DIR);
        Self {
            project_root,
            config,
            stub_dir,
        }
    }

    /// Check if make processing should be enabled
    fn should_process(&self) -> bool {
        scan_root(&self.project_root, &self.config.scan).exists()
    }

    /// Get stub path for a Makefile
    fn get_stub_path(&self, makefile: &PathBuf) -> PathBuf {
        let relative = makefile.strip_prefix(&self.project_root).unwrap_or(makefile);
        let stub_name = format!(
            "{}.done",
            relative.display().to_string().replace(['/', '\\'], "_"),
        );
        self.stub_dir.join(stub_name)
    }

    /// Run make in the Makefile's directory and create stub on success
    fn execute_make(&self, makefile: &PathBuf, stub_path: &PathBuf) -> Result<()> {
        let makefile_dir = makefile.parent()
            .context("Makefile has no parent directory")?;

        let mut cmd = Command::new(&self.config.make);

        for arg in &self.config.args {
            cmd.arg(arg);
        }

        if !self.config.target.is_empty() {
            cmd.arg(&self.config.target);
        }

        cmd.current_dir(makefile_dir);
        log_command(&cmd);

        let output = cmd
            .output()
            .context(format!("Failed to run {}", self.config.make))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            return Err(anyhow::anyhow!(
                "make failed in {}:\n{}{}",
                makefile_dir.display(),
                stdout,
                stderr
            ));
        }

        write_stub(stub_path, "make completed")
    }
}

impl ProductDiscovery for MakeProcessor {
    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        self.should_process() && !file_index.scan(&self.project_root, &self.config.scan, true).is_empty()
    }

    fn required_tools(&self) -> Vec<String> {
        vec![self.config.make.clone()]
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex) -> Result<()> {
        if !self.should_process() {
            return Ok(());
        }

        let makefiles = file_index.scan(&self.project_root, &self.config.scan, true);
        if makefiles.is_empty() {
            return Ok(());
        }

        let cfg_hash = Some(config_hash(&self.config));
        let extra = resolve_extra_inputs(&self.project_root, &self.config.extra_inputs)?;

        for makefile in makefiles {
            let stub_path = self.get_stub_path(&makefile);
            let mut inputs = vec![makefile];
            inputs.extend(extra.clone());
            graph.add_product(inputs, vec![stub_path], "make", cfg_hash.clone())?;
        }

        Ok(())
    }

    fn execute(&self, product: &Product) -> Result<()> {
        validate_stub_product(product, "Make")?;
        ensure_stub_dir(&self.stub_dir, "make")?;
        self.execute_make(&product.inputs[0], &product.outputs[0])
    }

    fn clean(&self, product: &Product) -> Result<()> {
        clean_outputs(product, "make")
    }
}
