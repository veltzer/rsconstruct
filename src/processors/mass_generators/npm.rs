use anyhow::Result;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::{NpmConfig, output_config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProductDiscovery, ProcessorType, SiblingFilter, scan_root_valid, run_in_anchor_dir, anchor_display_dir, check_command_output};

pub struct NpmProcessor {
    config: NpmConfig,
}

impl NpmProcessor {
    pub fn new(config: NpmConfig) -> Self {
        Self { config }
    }

    /// Run npm install in the package.json's directory
    fn execute_npm(&self, package_json: &Path) -> Result<()> {
        let mut cmd = Command::new(&self.config.npm);
        cmd.arg(&self.config.command);
        for arg in &self.config.args {
            cmd.arg(arg);
        }
        let output = run_in_anchor_dir(&mut cmd, package_json)?;
        check_command_output(&output, format_args!("npm {} in {}", self.config.command, anchor_display_dir(package_json)))
    }
}

impl ProductDiscovery for NpmProcessor {
    fn description(&self) -> &str {
        "Install Node.js dependencies using npm"
    }

    fn processor_type(&self) -> ProcessorType {
        ProcessorType::MassGenerator
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        scan_root_valid(&self.config.scan) && !file_index.scan(&self.config.scan, true).is_empty()
    }

    fn required_tools(&self) -> Vec<String> {
        vec![self.config.npm.clone(), "node".to_string()]
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex) -> Result<()> {
        let Some(files) = crate::processors::scan_or_skip(&self.config.scan, file_index) else {
            return Ok(());
        };

        let hash = Some(output_config_hash(&self.config, &[]));
        let extra = resolve_extra_inputs(&self.config.extra_inputs)?;

        let siblings = SiblingFilter {
            extensions: &[".json", ".js", ".ts"],
            excludes: &["/.git/", "/out/", "/.rsconstruct/", "/node_modules/"],
        };

        for anchor in files {
            let anchor_dir = anchor.parent().map(|p| p.to_path_buf()).unwrap_or_default();

            let sibling_files = file_index.query(
                &anchor_dir,
                siblings.extensions,
                siblings.excludes,
                &[],
                &[],
            );

            let inputs = crate::processors::build_anchor_inputs(&anchor, &sibling_files, &extra);

            if self.config.cache_output_dir {
                let output_dir = if anchor_dir.as_os_str().is_empty() {
                    PathBuf::from("node_modules")
                } else {
                    anchor_dir.join("node_modules")
                };
                graph.add_product_with_output_dir(inputs, vec![], crate::processors::names::NPM, hash.clone(), output_dir)?;
            } else {
                graph.add_product(inputs, vec![], crate::processors::names::NPM, hash.clone())?;
            }
        }

        Ok(())
    }

    fn execute(&self, product: &Product) -> Result<()> {
        self.execute_npm(product.primary_input())
    }

    fn clean(&self, product: &Product, verbose: bool) -> Result<usize> {
        crate::processors::clean_output_dir(product, crate::processors::names::NPM, verbose)
    }

    fn config_json(&self) -> Option<String> {
        serde_json::to_string(&self.config).ok()
    }

    fn max_jobs(&self) -> Option<usize> {
        self.config.max_jobs
    }
}
