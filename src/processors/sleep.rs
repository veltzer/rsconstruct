use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::config::{SleepConfig, config_hash, resolve_extra_inputs};
use crate::graph::{BuildGraph, Product};
use crate::ignore::IgnoreRules;
use super::{ProductDiscovery, find_files};

const SLEEP_STUB_DIR: &str = "out/sleep";

pub struct SleepProcessor {
    project_root: PathBuf,
    stub_dir: PathBuf,
    config: SleepConfig,
    ignore_rules: Arc<IgnoreRules>,
}

impl SleepProcessor {
    pub fn new(project_root: PathBuf, config: SleepConfig, ignore_rules: Arc<IgnoreRules>) -> Self {
        let stub_dir = project_root.join(SLEEP_STUB_DIR);
        Self {
            project_root,
            stub_dir,
            config,
            ignore_rules,
        }
    }

    /// Get the sleep directory from scan config
    fn sleep_dir(&self) -> PathBuf {
        let scan_dir = self.config.scan.scan_dir_or("sleep");
        if scan_dir.is_empty() {
            self.project_root.clone()
        } else {
            self.project_root.join(&scan_dir)
        }
    }

    /// Check if sleep processing should be enabled
    fn should_process(&self) -> bool {
        self.sleep_dir().exists()
    }

    /// Find all .sleep files
    fn find_sleep_files(&self) -> Vec<PathBuf> {
        let scan = &self.config.scan;
        let extensions = scan.extensions_or(&[".sleep"]);
        let exclude_dirs = scan.exclude_dirs_or(&[]);
        let ext_refs: Vec<&str> = extensions.iter().map(|s| s.as_str()).collect();
        let exclude_refs: Vec<&str> = exclude_dirs.iter().map(|s| s.as_str()).collect();
        find_files(&self.sleep_dir(), &ext_refs, &exclude_refs, &self.ignore_rules, true)
    }

    /// Get stub path for a sleep file
    fn get_stub_path(&self, sleep_file: &PathBuf) -> PathBuf {
        let file_stem = sleep_file
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown");
        self.stub_dir.join(format!("{}.done", file_stem))
    }

    /// Read duration from sleep file and sleep
    fn execute_sleep(&self, sleep_file: &PathBuf, stub_path: &PathBuf) -> Result<()> {
        // Read duration from file
        let content = fs::read_to_string(sleep_file)
            .context(format!("Failed to read sleep file: {}", sleep_file.display()))?;

        let duration_secs: f64 = content
            .trim()
            .parse()
            .context(format!("Invalid duration in {}: '{}'", sleep_file.display(), content.trim()))?;

        // Sleep for the specified duration
        let duration = Duration::from_secs_f64(duration_secs);
        thread::sleep(duration);

        // Create stub file on success
        if let Some(parent) = stub_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(stub_path, format!("slept for {} seconds", duration_secs))
            .context("Failed to create sleep stub file")?;

        Ok(())
    }
}

impl ProductDiscovery for SleepProcessor {
    fn auto_detect(&self) -> bool {
        self.should_process()
    }

    fn discover(&self, graph: &mut BuildGraph) -> Result<()> {
        if !self.should_process() {
            return Ok(());
        }

        let sleep_files = self.find_sleep_files();
        let cfg_hash = Some(config_hash(&self.config));
        let extra = resolve_extra_inputs(&self.project_root, &self.config.extra_inputs)?;

        for sleep_file in sleep_files {
            let stub_path = self.get_stub_path(&sleep_file);
            let mut inputs = vec![sleep_file];
            inputs.extend(extra.clone());
            graph.add_product(
                inputs,
                vec![stub_path],
                "sleep",
                cfg_hash.clone(),
            );
        }

        Ok(())
    }

    fn execute(&self, product: &Product) -> Result<()> {
        if product.inputs.is_empty() || product.outputs.len() != 1 {
            anyhow::bail!("Sleep product must have at least one input and exactly one output");
        }

        // Ensure stub directory exists
        if !self.stub_dir.exists() {
            fs::create_dir_all(&self.stub_dir)
                .context("Failed to create sleep stub directory")?;
        }

        self.execute_sleep(&product.inputs[0], &product.outputs[0])
    }

    fn clean(&self, product: &Product) -> Result<()> {
        for output in &product.outputs {
            if output.exists() {
                fs::remove_file(output)?;
                println!("Removed sleep stub: {}", output.display());
            }
        }
        Ok(())
    }
}
