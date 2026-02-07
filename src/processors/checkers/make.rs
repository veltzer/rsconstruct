use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::{MakeConfig, config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProductDiscovery, scan_root, run_command, check_command_output};

pub struct MakeProcessor {
    config: MakeConfig,
}

impl MakeProcessor {
    pub fn new(_project_root: PathBuf, config: MakeConfig) -> Self {
        Self {
            config,
        }
    }

    /// Check if make processing should be enabled
    fn should_process(&self) -> bool {
        scan_root(&self.config.scan).as_os_str().is_empty() || scan_root(&self.config.scan).exists()
    }

    /// Run make in the Makefile's directory
    fn execute_make(&self, makefile: &Path) -> Result<()> {
        let makefile_dir = makefile.parent()
            .context("Makefile has no parent directory")?;

        let mut cmd = Command::new(&self.config.make);

        for arg in &self.config.args {
            cmd.arg(arg);
        }

        if !self.config.target.is_empty() {
            cmd.arg(&self.config.target);
        }

        // Only set current_dir if not empty (root-level Makefile)
        if !makefile_dir.as_os_str().is_empty() {
            cmd.current_dir(makefile_dir);
        }

        let output = run_command(&mut cmd)?;
        let display_dir = if makefile_dir.as_os_str().is_empty() { "." } else { &makefile_dir.to_string_lossy() };
        check_command_output(&output, format_args!("make in {}", display_dir))
    }
}

impl ProductDiscovery for MakeProcessor {
    fn description(&self) -> &str {
        "Run make in directories containing Makefiles"
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        self.should_process() && !file_index.scan(&self.config.scan, true).is_empty()
    }

    fn required_tools(&self) -> Vec<String> {
        vec![self.config.make.clone()]
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex) -> Result<()> {
        if !self.should_process() {
            return Ok(());
        }

        let makefiles = file_index.scan(&self.config.scan, true);
        if makefiles.is_empty() {
            return Ok(());
        }

        let cfg_hash = Some(config_hash(&self.config));
        let extra = resolve_extra_inputs(&self.config.extra_inputs)?;

        for makefile in makefiles {
            let makefile_dir = makefile.parent().map(|p| p.to_path_buf()).unwrap_or_default();

            // Collect all files under the Makefile's directory as inputs so that
            // changes to any sibling source file trigger a rebuild.
            let sibling_files = file_index.query(
                &makefile_dir,
                &[""],       // match all extensions
                &["/.git/", "/out/", "/.rsb/"],
                &[],
                &[],
            );

            let mut inputs: Vec<PathBuf> = Vec::new();
            // Makefile first so product display shows it
            inputs.push(makefile.clone());
            for file in &sibling_files {
                if *file != makefile {
                    inputs.push(file.clone());
                }
            }
            inputs.extend(extra.clone());
            // Empty outputs: cache entry = success record
            graph.add_product(inputs, vec![], "make", cfg_hash.clone())?;
        }

        Ok(())
    }

    fn execute(&self, product: &Product) -> Result<()> {
        self.execute_make(&product.inputs[0])
    }
}
