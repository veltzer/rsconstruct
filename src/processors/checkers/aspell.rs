use anyhow::{Context, Result};
use std::process::{Command, Stdio};
use std::io::Write;

use crate::config::AspellConfig;
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProductDiscovery, scan_root_valid, log_command, format_command};

pub struct AspellProcessor {
    config: AspellConfig,
}

impl AspellProcessor {
    pub fn new(config: AspellConfig) -> Self {
        Self { config }
    }

    fn should_process(&self) -> bool {
        scan_root_valid(&self.config.scan)
    }

    fn check_file(&self, file: &std::path::Path) -> Result<()> {
        let content = std::fs::read_to_string(file)
            .with_context(|| format!("Failed to read file: {}", file.display()))?;

        let mut cmd = Command::new(&self.config.aspell);
        cmd.arg("--conf-dir").arg(&self.config.conf_dir);
        cmd.arg("--conf").arg(&self.config.conf);
        for arg in &self.config.args {
            cmd.arg(arg);
        }
        cmd.arg("list");
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        log_command(&cmd);

        let mut child = cmd.spawn()
            .with_context(|| format!("Failed to spawn: {}", format_command(&cmd)))?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(content.as_bytes())
                .context("Failed to write to aspell stdin")?;
        }

        let output = child.wait_with_output()
            .context("Failed to wait for aspell")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("aspell failed for {}: {}", file.display(), stderr.trim_end());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let misspelled: Vec<&str> = stdout.lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .collect();

        if !misspelled.is_empty() {
            anyhow::bail!(
                "Misspelled words in {}:\n{}",
                file.display(),
                misspelled.join("\n"),
            );
        }

        Ok(())
    }
}

impl ProductDiscovery for AspellProcessor {
    fn description(&self) -> &str {
        "Check spelling using aspell"
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        self.should_process() && !file_index.scan(&self.config.scan, true).is_empty()
    }

    fn required_tools(&self) -> Vec<String> {
        vec![self.config.aspell.clone()]
    }

    fn discover(
        &self,
        graph: &mut BuildGraph,
        file_index: &FileIndex,
    ) -> Result<()> {
        if !self.should_process() {
            return Ok(());
        }

        crate::processors::discover_checker_products(
            graph,
            &self.config.scan,
            file_index,
            &self.config.extra_inputs,
            &self.config,
            crate::processors::names::ASPELL,
        )
    }

    fn execute(&self, product: &Product) -> Result<()> {
        self.check_file(product.primary_input())
    }

    fn config_json(&self) -> Option<String> {
        serde_json::to_string(&self.config).ok()
    }
}
