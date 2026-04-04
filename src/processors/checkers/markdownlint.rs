use anyhow::Result;
use std::path::PathBuf;
use std::process::Command;

use crate::config::{MarkdownlintConfig, output_config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProductDiscovery, check_command_output, config_file_inputs, run_command};

pub struct MarkdownlintProcessor {
    config: MarkdownlintConfig,
}

impl MarkdownlintProcessor {
    pub fn new(config: MarkdownlintConfig) -> Self {
        Self { config }
    }
}

impl ProductDiscovery for MarkdownlintProcessor {
    fn description(&self) -> &str {
        "Lint Markdown files using markdownlint"
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        !file_index.scan(&self.config.scan, true).is_empty()
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex) -> Result<()> {
        let files = file_index.scan(&self.config.scan, true);
        if files.is_empty() {
            return Ok(());
        }
        let hash = Some(output_config_hash(&self.config, &[]));
        let mut extra_inputs = self.config.extra_inputs.clone();
        for ai in &self.config.auto_inputs {
            extra_inputs.extend(config_file_inputs(ai));
        }
        let extra = resolve_extra_inputs(&extra_inputs)?;

        // Only depend on the npm stamp when using a local repo install
        let npm_stamp = if self.config.local_repo {
            Some(PathBuf::from(&self.config.npm_stamp))
        } else {
            None
        };

        for file in files {
            let mut inputs = Vec::with_capacity(1 + extra.len() + 1);
            inputs.push(file);
            inputs.extend_from_slice(&extra);
            if let Some(ref stamp) = npm_stamp {
                inputs.push(stamp.clone());
            }
            graph.add_product(inputs, vec![], crate::processors::names::MARKDOWNLINT, hash.clone())?;
        }
        Ok(())
    }

    fn required_tools(&self) -> Vec<String> {
        vec![self.config.markdownlint_bin.clone()]
    }

    fn execute(&self, product: &Product) -> Result<()> {
        let file = product.primary_input();
        let mut cmd = Command::new(&self.config.markdownlint_bin);
        for arg in &self.config.args {
            cmd.arg(arg);
        }
        cmd.arg(file);
        let output = run_command(&mut cmd)?;
        check_command_output(&output, format_args!("markdownlint {}", file.display()))
    }

    fn config_json(&self) -> Option<String> {
        serde_json::to_string(&self.config).ok()
    }

    fn max_jobs(&self) -> Option<usize> {
        self.config.max_jobs
    }
}
