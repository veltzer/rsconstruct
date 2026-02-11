use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

use crate::config::MakeConfig;
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProductDiscovery, SiblingFilter, discover_directory_products, scan_root_valid, run_command, check_command_output};

pub struct MakeProcessor {
    config: MakeConfig,
}

impl MakeProcessor {
    pub fn new(config: MakeConfig) -> Self {
        Self {
            config,
        }
    }

    /// Check if make processing should be enabled
    fn should_process(&self) -> bool {
        scan_root_valid(&self.config.scan)
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

        discover_directory_products(
            graph,
            &self.config.scan,
            file_index,
            &self.config.extra_inputs,
            &self.config,
            &SiblingFilter {
                extensions: &[""],       // match all extensions
                excludes: &["/.git/", "/out/", "/.rsb/"],
            },
            "make",
        )
    }

    fn config_json(&self) -> Option<String> {
        serde_json::to_string(&self.config).ok()
    }

    fn execute(&self, product: &Product) -> Result<()> {
        self.execute_make(&product.inputs[0])
    }
}
