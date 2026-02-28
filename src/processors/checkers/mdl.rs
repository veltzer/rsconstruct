use anyhow::Result;
use std::path::PathBuf;
use std::process::Command;

use crate::config::{MdlConfig, config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProductDiscovery, check_command_output, run_command};

pub struct MdlProcessor {
    config: MdlConfig,
}

impl MdlProcessor {
    pub fn new(config: MdlConfig) -> Self {
        Self { config }
    }
}

impl ProductDiscovery for MdlProcessor {
    fn description(&self) -> &str {
        "Lint Markdown files using mdl (markdownlint)"
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        !file_index.scan(&self.config.scan, true).is_empty()
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex) -> Result<()> {
        let files = file_index.scan(&self.config.scan, true);
        if files.is_empty() {
            return Ok(());
        }
        let hash = Some(config_hash(&self.config));
        let extra = resolve_extra_inputs(&self.config.extra_inputs)?;

        // The gem stamp path is added directly (without existence validation)
        // so that resolve_dependencies() can create the dependency edge to the
        // gem processor's output.
        let gem_stamp = PathBuf::from(&self.config.gem_stamp);

        for file in files {
            let mut inputs = Vec::with_capacity(1 + extra.len() + 1);
            inputs.push(file);
            inputs.extend_from_slice(&extra);
            inputs.push(gem_stamp.clone());
            graph.add_product(inputs, vec![], crate::processors::names::MDL, hash.clone())?;
        }
        Ok(())
    }

    fn required_tools(&self) -> Vec<String> {
        vec![self.config.mdl_bin.clone(), "ruby".to_string()]
    }

    fn execute(&self, product: &Product) -> Result<()> {
        let file = product.primary_input();
        let mut cmd = Command::new(&self.config.mdl_bin);
        for arg in &self.config.args {
            cmd.arg(arg);
        }
        cmd.arg(file);
        cmd.env("GEM_HOME", &self.config.gem_home);
        cmd.env("GEM_PATH", &self.config.gem_home);
        let output = run_command(&mut cmd)?;
        check_command_output(&output, format_args!("mdl {}", file.display()))
    }

    fn config_json(&self) -> Option<String> {
        serde_json::to_string(&self.config).ok()
    }
}
