use anyhow::Result;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::{GemConfig, output_config_hash, resolve_extra_inputs};
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
        let Some(files) = crate::processors::scan_or_skip(&self.config.scan, file_index) else {
            return Ok(());
        };

        let hash = Some(output_config_hash(&self.config, &[]));
        let extra = resolve_extra_inputs(&self.config.extra_inputs)?;

        let siblings = SiblingFilter {
            extensions: &[".gemspec"],
            excludes: &["/.git/", "/out/", "/.rsconstruct/", "/gems/"],
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
        crate::processors::clean_output_dir(product, crate::processors::names::GEM, verbose)
    }

    fn config_json(&self) -> Option<String> {
        serde_json::to_string(&self.config).ok()
    }

    fn max_jobs(&self) -> Option<usize> {
        self.config.max_jobs
    }
}
