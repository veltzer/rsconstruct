use anyhow::{Context, Result};
use std::collections::HashSet;
use std::path::Path;
use std::process::{Command, Stdio};
use std::io::Write;

use crate::config::AspellConfig;
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProductDiscovery, config_file_inputs, scan_root_valid, log_command, format_command};
use crate::processors::word_manager::WordManager;

pub struct AspellProcessor {
    config: AspellConfig,
    words: WordManager,
}

impl AspellProcessor {
    pub fn new(config: AspellConfig) -> Self {
        let custom_words = Self::load_custom_words(Path::new(&config.words_file));
        let words = WordManager::new(
            custom_words,
            config.words_file.clone(),
            Some("personal_ws-1.1 en 0"),
        );
        Self { config, words }
    }

    /// Load custom words from the aspell personal word list (.pws) file
    fn load_custom_words(words_path: &Path) -> HashSet<String> {
        if !words_path.exists() {
            return HashSet::new();
        }
        std::fs::read_to_string(words_path)
            .map(|content| {
                content
                    .lines()
                    .filter(|l| !l.starts_with("personal_ws"))
                    .map(|l| l.trim().to_lowercase())
                    .filter(|l| !l.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn check_file(&self, file: &Path) -> Result<()> {
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
            .filter(|l| !self.words.is_known(&l.to_lowercase()))
            .collect();

        self.words.handle_misspelled(&misspelled, file, self.config.auto_add_words)
    }
}

impl ProductDiscovery for AspellProcessor {
    fn description(&self) -> &str {
        "Check spelling using aspell"
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        scan_root_valid(&self.config.scan) && !file_index.scan(&self.config.scan, true).is_empty()
    }

    fn required_tools(&self) -> Vec<String> {
        vec![self.config.aspell.clone()]
    }

    fn discover(
        &self,
        graph: &mut BuildGraph,
        file_index: &FileIndex,
    ) -> Result<()> {
        if !scan_root_valid(&self.config.scan) {
            return Ok(());
        }

        let mut extra_inputs = self.config.extra_inputs.clone();
        for ai in &self.config.auto_inputs {
            extra_inputs.extend(config_file_inputs(ai));
        }
        crate::processors::discover_checker_products(
            graph,
            &self.config.scan,
            file_index,
            &extra_inputs,
            &self.config,
            crate::processors::names::ASPELL,
        )
    }

    fn execute(&self, product: &Product) -> Result<()> {
        self.words.execute_with_flush(
            product,
            self.config.auto_add_words,
            |file| self.check_file(file),
            "aspell",
        )
    }

    fn supports_batch(&self) -> bool {
        self.config.auto_add_words
    }

    fn execute_batch(&self, products: &[&Product]) -> Vec<Result<()>> {
        self.words.execute_batch_with_flush(
            products,
            self.config.auto_add_words,
            |file| self.check_file(file),
            "aspell",
        )
    }

    fn config_json(&self) -> Option<String> {
        serde_json::to_string(&self.config).ok()
    }

    fn max_jobs(&self) -> Option<usize> {
        self.config.max_jobs
    }
}
