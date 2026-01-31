use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::config::{SleepConfig, config_hash, resolve_extra_inputs};
use crate::graph::{BuildGraph, Product};
use crate::ignore::IgnoreRules;
use super::{ProductDiscovery, scan_files, scan_root, validate_stub_product, ensure_stub_dir, write_stub, clean_outputs};

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

    /// Check if sleep processing should be enabled
    fn should_process(&self) -> bool {
        scan_root(&self.project_root, &self.config.scan).exists()
    }

    /// Get stub path for a sleep file (uses file stem, not full relative path)
    fn get_stub_path(&self, sleep_file: &PathBuf) -> PathBuf {
        let file_stem = sleep_file
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown");
        self.stub_dir.join(format!("{}.done", file_stem))
    }

    /// Read duration from sleep file and sleep
    fn execute_sleep(&self, sleep_file: &PathBuf, stub_path: &PathBuf) -> Result<()> {
        let content = fs::read_to_string(sleep_file)
            .context(format!("Failed to read sleep file: {}", sleep_file.display()))?;

        let duration_secs: f64 = content
            .trim()
            .parse()
            .context(format!("Invalid duration in {}: '{}'", sleep_file.display(), content.trim()))?;

        let duration = Duration::from_secs_f64(duration_secs);
        thread::sleep(duration);

        write_stub(stub_path, &format!("slept for {} seconds", duration_secs))
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

        let sleep_files = scan_files(&self.project_root, &self.config.scan, &self.ignore_rules, true);
        if sleep_files.is_empty() {
            return Ok(());
        }

        let cfg_hash = Some(config_hash(&self.config));
        let extra = resolve_extra_inputs(&self.project_root, &self.config.extra_inputs)?;

        for sleep_file in sleep_files {
            let stub_path = self.get_stub_path(&sleep_file);
            let mut inputs = vec![sleep_file];
            inputs.extend(extra.clone());
            graph.add_product(inputs, vec![stub_path], "sleep", cfg_hash.clone());
        }

        Ok(())
    }

    fn execute(&self, product: &Product) -> Result<()> {
        validate_stub_product(product, "Sleep")?;
        ensure_stub_dir(&self.stub_dir, "sleep")?;
        self.execute_sleep(&product.inputs[0], &product.outputs[0])
    }

    fn clean(&self, product: &Product) -> Result<()> {
        clean_outputs(product, "sleep")
    }
}
