use anyhow::Result;
use std::path::Path;
use std::process::Command;

use crate::config::SphinxConfig;
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProductDiscovery, ProcessorType, SiblingFilter, discover_directory_products, scan_root_valid, run_in_anchor_dir, anchor_display_dir, check_command_output};

pub struct SphinxProcessor {
    config: SphinxConfig,
}

impl SphinxProcessor {
    pub fn new(config: SphinxConfig) -> Self {
        Self { config }
    }

    fn should_process(&self) -> bool {
        scan_root_valid(&self.config.scan)
    }

    /// Run sphinx-build in the conf.py's directory
    fn execute_sphinx(&self, conf_py: &Path) -> Result<()> {
        let mut cmd = Command::new(&self.config.sphinx_build);
        // Source dir is the directory containing conf.py
        cmd.arg(".");
        // Output dir
        cmd.arg(&self.config.output_dir);
        for arg in &self.config.args {
            cmd.arg(arg);
        }
        let output = run_in_anchor_dir(&mut cmd, conf_py)?;
        check_command_output(&output, format_args!("sphinx-build in {}", anchor_display_dir(conf_py)))
    }
}

impl ProductDiscovery for SphinxProcessor {
    fn description(&self) -> &str {
        "Build Sphinx documentation"
    }

    fn processor_type(&self) -> ProcessorType {
        ProcessorType::MassGenerator
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        self.should_process() && !file_index.scan(&self.config.scan, true).is_empty()
    }

    fn required_tools(&self) -> Vec<String> {
        vec![self.config.sphinx_build.clone(), "python3".to_string()]
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
                extensions: &[".rst", ".py", ".md"],
                excludes: &["/.git/", "/out/", "/.rsb/", "/_build/"],
            },
            crate::processors::names::SPHINX,
        )
    }

    fn execute(&self, product: &Product) -> Result<()> {
        self.execute_sphinx(product.primary_input())
    }

    fn config_json(&self) -> Option<String> {
        serde_json::to_string(&self.config).ok()
    }
}
