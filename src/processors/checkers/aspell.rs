use anyhow::{Context, Result};
use std::collections::HashSet;
use std::path::Path;
use std::process::{Command, Stdio};
use std::io::Write;
use parking_lot::Mutex;

use crate::config::AspellConfig;
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProductDiscovery, config_file_inputs, scan_root_valid, log_command, format_command};

pub struct AspellProcessor {
    config: AspellConfig,
    /// Custom words, loaded once at initialization
    custom_words: HashSet<String>,
    /// Words to add to the words file (collected during auto_add_words mode)
    words_to_add: Mutex<HashSet<String>>,
}

impl AspellProcessor {
    pub fn new(config: AspellConfig) -> Self {
        let custom_words = Self::load_custom_words(Path::new(&config.words_file));
        Self {
            config,
            custom_words,
            words_to_add: Mutex::new(HashSet::new()),
        }
    }

    fn should_process(&self) -> bool {
        scan_root_valid(&self.config.scan)
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
            .filter(|l| !self.custom_words.contains(&l.to_lowercase()))
            .collect();

        if !misspelled.is_empty() {
            if self.config.auto_add_words {
                let mut words_to_add = self.words_to_add.lock();
                for word in &misspelled {
                    words_to_add.insert(word.to_lowercase());
                }
                Ok(())
            } else {
                anyhow::bail!(
                    "Misspelled words in {}:\n{}",
                    file.display(),
                    misspelled.join("\n"),
                );
            }
        } else {
            Ok(())
        }
    }

    /// Write collected words to the aspell personal word list (.pws) file
    fn flush_words_to_file(&self) -> Result<()> {
        let words_to_add = self.words_to_add.lock();
        if words_to_add.is_empty() {
            return Ok(());
        }

        let words_path = Path::new(&self.config.words_file);

        // Read existing words if file exists (skip the pws header line)
        let mut all_words: HashSet<String> = Self::load_custom_words(words_path);

        let new_count = words_to_add.iter().filter(|w| !all_words.contains(*w)).count();
        for word in words_to_add.iter() {
            all_words.insert(word.clone());
        }

        if new_count == 0 {
            return Ok(());
        }

        let mut sorted: Vec<_> = all_words.into_iter().collect();
        sorted.sort();

        let mut file = std::fs::File::create(words_path)
            .with_context(|| format!("Failed to create words file: {}", words_path.display()))?;
        writeln!(file, "personal_ws-1.1 en {}", sorted.len())?;
        for word in &sorted {
            writeln!(file, "{}", word)?;
        }

        println!("Added {} word(s) to {}", new_count, words_path.display());
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
        let result = self.check_file(product.primary_input());
        if self.config.auto_add_words
            && let Err(e) = self.flush_words_to_file()
        {
            eprintln!("Warning: failed to flush aspell words file: {}", e);
        }
        result
    }

    fn supports_batch(&self) -> bool {
        self.config.auto_add_words
    }

    fn execute_batch(&self, products: &[&Product]) -> Vec<Result<()>> {
        let results: Vec<Result<()>> = products
            .iter()
            .map(|p| self.check_file(p.primary_input()))
            .collect();

        if self.config.auto_add_words
            && let Err(e) = self.flush_words_to_file()
        {
            eprintln!("Warning: failed to flush aspell words file: {}", e);
        }

        results
    }

    fn config_json(&self) -> Option<String> {
        serde_json::to_string(&self.config).ok()
    }
}
