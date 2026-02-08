use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use crate::config::SleepConfig;
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProductDiscovery, scan_root_valid, discover_checker_products, is_interrupted};

pub struct SleepProcessor {
    config: SleepConfig,
}

impl SleepProcessor {
    pub fn new(_project_root: PathBuf, config: SleepConfig) -> Self {
        Self {
            config,
        }
    }

    /// Check if sleep processing should be enabled
    fn should_process(&self) -> bool {
        scan_root_valid(&self.config.scan)
    }

    /// Read duration from sleep file and sleep
    fn execute_sleep(&self, sleep_file: &Path) -> Result<()> {
        let content = fs::read_to_string(sleep_file)
            .context(format!("Failed to read sleep file: {}", sleep_file.display()))?;

        let duration_secs: f64 = content
            .trim()
            .parse()
            .context(format!("Invalid duration in {}: '{}'", sleep_file.display(), content.trim()))?;

        let total = Duration::from_secs_f64(duration_secs);
        let interval = Duration::from_millis(50);
        let mut elapsed = Duration::ZERO;
        while elapsed < total {
            if is_interrupted() {
                anyhow::bail!("Interrupted");
            }
            let remaining = total - elapsed;
            let sleep_time = remaining.min(interval);
            thread::sleep(sleep_time);
            elapsed += sleep_time;
        }

        Ok(())
    }
}

impl ProductDiscovery for SleepProcessor {
    fn description(&self) -> &str {
        "Sleep for a duration (testing)"
    }

    fn hidden(&self) -> bool {
        true
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        self.should_process() && !file_index.scan(&self.config.scan, true).is_empty()
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex) -> Result<()> {
        if !self.should_process() {
            return Ok(());
        }
        discover_checker_products(
            graph,
            &self.config.scan,
            file_index,
            &self.config.extra_inputs,
            &self.config,
            "sleep",
        )
    }

    fn execute(&self, product: &Product) -> Result<()> {
        self.execute_sleep(&product.inputs[0])
    }
}
