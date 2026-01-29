use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use walkdir::WalkDir;

use crate::config::{SleepConfig, config_hash, resolve_extra_inputs};
use crate::graph::{BuildGraph, Product};
use crate::ignore::IgnoreRules;
use super::ProductDiscovery;

const SLEEP_DIR: &str = "sleep";
const SLEEP_STUB_DIR: &str = "out/sleep";

pub struct SleepProcessor {
    project_root: PathBuf,
    sleep_dir: PathBuf,
    stub_dir: PathBuf,
    config: SleepConfig,
    ignore_rules: Arc<IgnoreRules>,
}

impl SleepProcessor {
    pub fn new(project_root: PathBuf, config: SleepConfig, ignore_rules: Arc<IgnoreRules>) -> Self {
        let sleep_dir = project_root.join(SLEEP_DIR);
        let stub_dir = project_root.join(SLEEP_STUB_DIR);
        Self {
            project_root,
            sleep_dir,
            stub_dir,
            config,
            ignore_rules,
        }
    }

    /// Check if sleep processing should be enabled
    fn should_process(&self) -> bool {
        self.sleep_dir.exists()
    }

    /// Find all .sleep files
    fn find_sleep_files(&self) -> Vec<PathBuf> {
        if !self.sleep_dir.exists() {
            return Vec::new();
        }

        let mut files: Vec<PathBuf> = WalkDir::new(&self.sleep_dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("sleep"))
            .map(|e| e.path().to_path_buf())
            .filter(|p| !self.ignore_rules.is_ignored(p))
            .collect();
        files.sort();
        files
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
