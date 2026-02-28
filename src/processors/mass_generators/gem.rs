use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::{GemConfig, config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProductDiscovery, ProcessorType, SiblingFilter, scan_root_valid, run_in_anchor_dir, anchor_display_dir, check_command_output};

pub struct GemProcessor {
    config: GemConfig,
    output_dir: PathBuf,
}

impl GemProcessor {
    pub fn new(config: GemConfig) -> Self {
        Self {
            config,
            output_dir: PathBuf::from("out/gem"),
        }
    }

    fn should_process(&self) -> bool {
        scan_root_valid(&self.config.scan)
    }

    /// Compute the stamp file path for a Gemfile anchor.
    fn stamp_path(&self, anchor: &Path) -> PathBuf {
        let anchor_dir = anchor.parent().unwrap_or(Path::new(""));
        let name = if anchor_dir.as_os_str().is_empty() {
            "root".to_string()
        } else {
            anchor_dir.display().to_string().replace(['/', '\\'], "_")
        };
        self.output_dir.join(format!("{}.stamp", name))
    }

    /// Run bundle install in the Gemfile's directory
    fn execute_gem(&self, gemfile: &Path) -> Result<()> {
        let mut cmd = Command::new(&self.config.bundler);
        cmd.arg(&self.config.command);
        cmd.env("GEM_HOME", &self.config.gem_home);
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
        self.should_process() && !file_index.scan(&self.config.scan, true).is_empty()
    }

    fn required_tools(&self) -> Vec<String> {
        vec![self.config.bundler.clone(), "ruby".to_string()]
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex) -> Result<()> {
        if !self.should_process() {
            return Ok(());
        }

        let files = file_index.scan(&self.config.scan, true);
        if files.is_empty() {
            return Ok(());
        }

        let hash = Some(config_hash(&self.config));
        let extra = resolve_extra_inputs(&self.config.extra_inputs)?;

        let siblings = SiblingFilter {
            extensions: &[".rb", ".gemspec"],
            excludes: &["/.git/", "/out/", "/.rsb/", "/vendor/"],
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

            let stamp = self.stamp_path(&anchor);

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
                graph.add_product_with_output_dir(inputs, vec![stamp], crate::processors::names::GEM, hash.clone(), output_dir)?;
            } else {
                graph.add_product(inputs, vec![stamp], crate::processors::names::GEM, hash.clone())?;
            }
        }

        Ok(())
    }

    fn execute(&self, product: &Product) -> Result<()> {
        self.execute_gem(product.primary_input())?;

        // Create stamp file
        let stamp = product.outputs.first()
            .context("gem product has no output stamp")?;
        if let Some(parent) = stamp.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create gem output directory: {}", parent.display()))?;
        }
        fs::write(stamp, "")
            .with_context(|| format!("Failed to write gem stamp file: {}", stamp.display()))?;
        Ok(())
    }

    fn clean(&self, product: &Product, verbose: bool) -> Result<usize> {
        let mut count = 0;
        for output in &product.outputs {
            match fs::remove_file(output) {
                Ok(()) => {
                    count += 1;
                    if verbose {
                        println!("Removed gem output: {}", output.display());
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        if let Some(ref output_dir) = product.output_dir
            && output_dir.exists()
        {
            if verbose {
                println!("Removing gem output directory: {}", output_dir.display());
            }
            fs::remove_dir_all(output_dir.as_ref())?;
            count += 1;
        }
        Ok(count)
    }

    fn config_json(&self) -> Option<String> {
        serde_json::to_string(&self.config).ok()
    }
}
