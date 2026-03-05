use anyhow::Result;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::{GemConfig, config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProductDiscovery, ProcessorType, SiblingFilter, scan_root_valid, run_in_anchor_dir, anchor_display_dir, check_command_output};

pub struct GemProcessor {
    config: GemConfig,
}

impl GemProcessor {
    pub fn new(config: GemConfig) -> Self {
        Self { config }
    }

    /// Run bundle install in the Gemfile's directory
    fn execute_gem(&self, gemfile: &Path) -> Result<()> {
        let mut cmd = Command::new(&self.config.bundler);
        cmd.arg(&self.config.command);
        cmd.env("GEM_HOME", &self.config.gem_home);
        cmd.env("GEM_PATH", &self.config.gem_home);
        for arg in &self.config.args {
            cmd.arg(arg);
        }
        let output = run_in_anchor_dir(&mut cmd, gemfile)?;
        check_command_output(&output, format_args!("bundle {} in {}", self.config.command, anchor_display_dir(gemfile)))
    }
}

impl ProductDiscovery for GemProcessor {
    fn description(&self) -> &str {
        "Install Ruby dependencies using Bundler"
    }

    fn processor_type(&self) -> ProcessorType {
        ProcessorType::MassGenerator
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        scan_root_valid(&self.config.scan) && !file_index.scan(&self.config.scan, true).is_empty()
    }

    fn required_tools(&self) -> Vec<String> {
        vec![self.config.bundler.clone(), "ruby".to_string()]
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex) -> Result<()> {
        if !scan_root_valid(&self.config.scan) {
            return Ok(());
        }

        let files = file_index.scan(&self.config.scan, true);
        if files.is_empty() {
            return Ok(());
        }

        let hash = Some(config_hash(&self.config));
        let extra = resolve_extra_inputs(&self.config.extra_inputs)?;

        let siblings = SiblingFilter {
            extensions: &[".gemspec"],
            excludes: &["/.git/", "/out/", "/.rsbuild/", "/gems/"],
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

            let mut inputs: Vec<PathBuf> = Vec::with_capacity(1 + sibling_files.len() + extra.len());
            inputs.push(anchor.clone());
            for file in &sibling_files {
                if *file != anchor {
                    inputs.push(file.clone());
                }
            }
            inputs.extend_from_slice(&extra);

            if self.config.cache_output_dir {
                let output_dir = if anchor_dir.as_os_str().is_empty() {
                    PathBuf::from(&self.config.gem_home)
                } else {
                    anchor_dir.join(&self.config.gem_home)
                };
                graph.add_product_with_output_dir(inputs, vec![], crate::processors::names::GEM, hash.clone(), output_dir)?;
            } else {
                graph.add_product(inputs, vec![], crate::processors::names::GEM, hash.clone())?;
            }
        }

        Ok(())
    }

    fn execute(&self, product: &Product) -> Result<()> {
        self.execute_gem(product.primary_input())
    }

    fn clean(&self, product: &Product, verbose: bool) -> Result<usize> {
        if let Some(ref output_dir) = product.output_dir
            && output_dir.exists()
        {
            if verbose {
                println!("Removing gem output directory: {}", output_dir.display());
            }
            std::fs::remove_dir_all(output_dir.as_ref())?;
            return Ok(1);
        }
        Ok(0)
    }

    fn config_json(&self) -> Option<String> {
        serde_json::to_string(&self.config).ok()
    }
}
