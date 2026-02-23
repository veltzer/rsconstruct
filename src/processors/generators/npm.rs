use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::{NpmConfig, config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProductDiscovery, ProcessorType, SiblingFilter, clean_outputs, scan_root_valid, run_in_anchor_dir, anchor_display_dir, check_command_output};

pub struct NpmProcessor {
    config: NpmConfig,
    output_dir: PathBuf,
}

impl NpmProcessor {
    pub fn new(config: NpmConfig) -> Self {
        Self {
            config,
            output_dir: PathBuf::from("out/npm"),
        }
    }

    fn should_process(&self) -> bool {
        scan_root_valid(&self.config.scan)
    }

    /// Compute the stamp file path for a package.json anchor.
    fn stamp_path(&self, anchor: &Path) -> PathBuf {
        let anchor_dir = anchor.parent().unwrap_or(Path::new(""));
        let name = if anchor_dir.as_os_str().is_empty() {
            "root".to_string()
        } else {
            anchor_dir.display().to_string().replace(['/', '\\'], "_")
        };
        self.output_dir.join(format!("{}.stamp", name))
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
        ProcessorType::Generator
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        self.should_process() && !file_index.scan(&self.config.scan, true).is_empty()
    }

    fn required_tools(&self) -> Vec<String> {
        vec![self.config.npm.clone()]
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
            extensions: &[".json", ".js", ".ts"],
            excludes: &["/.git/", "/out/", "/.rsb/", "/node_modules/"],
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

            graph.add_product(inputs, vec![stamp], crate::processors::names::NPM, hash.clone())?;
        }

        Ok(())
    }

    fn execute(&self, product: &Product) -> Result<()> {
        self.execute_npm(product.primary_input())?;

        // Create stamp file
        let stamp = product.outputs.first()
            .context("npm product has no output stamp")?;
        if let Some(parent) = stamp.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create npm output directory: {}", parent.display()))?;
        }
        fs::write(stamp, "")
            .with_context(|| format!("Failed to write npm stamp file: {}", stamp.display()))?;
        Ok(())
    }

    fn clean(&self, product: &Product, verbose: bool) -> Result<usize> {
        clean_outputs(product, crate::processors::names::NPM, verbose)
    }

    fn config_json(&self) -> Option<String> {
        serde_json::to_string(&self.config).ok()
    }
}
