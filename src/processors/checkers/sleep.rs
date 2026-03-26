use anyhow::{Context, Result};
use std::fs;
use std::path::Path;
use std::thread;
use std::time::Duration;

use crate::config::SleepConfig;
use crate::graph::Product;
use crate::processors::{scan_root_valid, is_interrupted};

pub struct SleepProcessor {
    config: SleepConfig,
}

impl SleepProcessor {
    pub fn new(config: SleepConfig) -> Self {
        Self {
            config,
        }
    }

    /// Check if sleep processing should be enabled
    fn should_process(&self) -> bool {
        scan_root_valid(&self.config.scan)
    }

    fn execute_product(&self, product: &Product) -> Result<()> {
        self.execute_sleep(product.primary_input())
    }

    /// Read duration from sleep file and sleep
    fn execute_sleep(&self, sleep_file: &Path) -> Result<()> {
        let content = fs::read_to_string(sleep_file)
            .with_context(|| format!("Failed to read sleep file: {}", sleep_file.display()))?;

        let duration_secs: f64 = content
            .trim()
            .parse()
            .with_context(|| format!("Invalid duration in {}: '{}'", sleep_file.display(), content.trim()))?;

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

impl_checker!(SleepProcessor,
    config: config,
    description: "Sleep for a duration (testing)",
    name: crate::processors::names::SLEEP,
    execute: execute_product,
    guard: should_process,
    hidden: true,
);
