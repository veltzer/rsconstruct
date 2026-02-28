use anyhow::{Context, Result};
use regex::Regex;
use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::Path;
use parking_lot::Mutex;
use std::sync::OnceLock;

use crate::config::SpellcheckConfig;
use crate::errors;
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProductDiscovery, config_file_inputs, discover_checker_products};

const DICT_DIR: &str = "/usr/share/hunspell";

pub struct SpellcheckProcessor {
    config: SpellcheckConfig,
    /// Cached dictionary, built once on first use and reused across all execute() calls
    cached_dict: OnceLock<Result<zspell::Dictionary, String>>,
    /// Custom words, loaded once at initialization
    custom_words: HashSet<String>,
    /// Words to add to the words file (collected during auto_add_words mode)
    words_to_add: Mutex<HashSet<String>>,
}

impl SpellcheckProcessor {
    pub fn new(config: SpellcheckConfig) -> Result<Self> {
        let custom_words = if config.use_words_file {
            let words_path = Path::new(&config.words_file);
            Self::load_custom_words(words_path)
                .with_context(|| format!("Custom words file not found: {}", words_path.display()))?
        } else {
            HashSet::new()
        };
        Ok(Self {
            config,
            cached_dict: OnceLock::new(),
            custom_words,
            words_to_add: Mutex::new(HashSet::new()),
        })
    }

    /// Load custom words from the words file
    fn load_custom_words(words_path: &Path) -> Result<HashSet<String>> {
        let content = fs::read_to_string(words_path)?;
        let mut words = HashSet::new();
        for line in content.lines() {
            let trimmed = line.trim();
            if !trimmed.is_empty() && !trimmed.starts_with('#') {
                words.insert(trimmed.to_lowercase());
            }
        }
        Ok(words)
    }

    /// Build a zspell Dictionary from system hunspell files
    fn build_dictionary(&self) -> Result<zspell::Dictionary> {
        let lang = &self.config.language;
        let aff_path = Path::new(DICT_DIR).join(format!("{}.aff", lang));
        let dic_path = Path::new(DICT_DIR).join(format!("{}.dic", lang));

        let aff_content = fs::read_to_string(&aff_path)
            .with_context(|| format!("Failed to read affix file: {}. Is the hunspell dictionary for '{}' installed?", aff_path.display(), lang))?;
        let dic_content = fs::read_to_string(&dic_path)
            .with_context(|| format!("Failed to read dictionary file: {}. Is the hunspell dictionary for '{}' installed?", dic_path.display(), lang))?;

        let dict = zspell::builder()
            .config_str(&aff_content)
            .dict_str(&dic_content)
            .build()
            .context("Failed to build spellcheck dictionary")?;

        Ok(dict)
    }

    /// Get or build the cached dictionary (built once, reused across all files)
    fn get_dictionary(&self) -> Result<&zspell::Dictionary> {
        let result = self.cached_dict.get_or_init(|| {
            self.build_dictionary().map_err(|e| e.to_string())
        });
        match result {
            Ok(dict) => Ok(dict),
            Err(msg) => anyhow::bail!("{}", msg),
        }
    }

    /// Extract words from markdown text, stripping code blocks, inline code, URLs, and HTML tags
    fn extract_words(text: &str) -> Vec<String> {
        let mut result = Vec::new();
        let mut in_fenced_block = false;

        for line in text.lines() {
            let trimmed = line.trim();

            // Toggle fenced code blocks
            if trimmed.starts_with("```") {
                in_fenced_block = !in_fenced_block;
                continue;
            }
            if in_fenced_block {
                continue;
            }

            // Skip indented code blocks (4 spaces or 1 tab)
            if line.starts_with("    ") || line.starts_with('\t') {
                continue;
            }

            let cleaned = Self::strip_markdown(line);

            // Split on non-alphabetic characters and collect words
            for word in cleaned.split(|c: char| !c.is_alphabetic()) {
                if word.len() >= 2 {
                    result.push(word.to_string());
                }
            }
        }

        result
    }

    /// Strip markdown syntax from a line using a single regex pass.
    fn strip_markdown(line: &str) -> String {
        static MARKDOWN_RE: OnceLock<Regex> = OnceLock::new();
        let re = MARKDOWN_RE.get_or_init(|| Regex::new(concat!(
            r"`[^`]*`",                          // inline code spans
            r"|\[([^\]]*)\]\([^)]*\)",            // [text](url) — capture group 1 = link text
            r#"|https?://[^\s)>""]+"#,            // bare URLs
            r"|<[^>]*>",                          // HTML tags
        )).expect(errors::INVALID_REGEX));

        re.replace_all(line, |caps: &regex::Captures| {
            // For markdown links, keep the link text; for everything else, replace with space
            caps.get(1).map_or(" ".to_string(), |m| m.as_str().to_string())
        }).into_owned()
    }

    /// Check a single file for spelling errors
    fn check_file(&self, doc_file: &Path) -> Result<()> {
        let dict = self.get_dictionary()?;
        let custom_words = &self.custom_words;

        let content = fs::read_to_string(doc_file)
            .with_context(|| format!("Failed to read document file: {}", doc_file.display()))?;

        let words = Self::extract_words(&content);
        let mut misspelled: Vec<String> = Vec::new();
        let mut seen = HashSet::new();

        for word in &words {
            let lower = word.to_lowercase();
            if seen.contains(&lower) {
                continue;
            }
            seen.insert(lower.clone());

            // Skip if in custom words
            if custom_words.contains(&lower) {
                continue;
            }

            // Check against dictionary
            if !dict.check_word(&lower) {
                misspelled.push(word.clone());
            }
        }

        if !misspelled.is_empty() {
            if self.config.auto_add_words {
                // Collect words to add to the words file
                let mut words_to_add = self.words_to_add.lock();
                for word in &misspelled {
                    words_to_add.insert(word.to_lowercase());
                }
                Ok(())
            } else {
                misspelled.sort();
                Err(anyhow::anyhow!(
                    "Spelling errors in {}: {}",
                    doc_file.display(),
                    misspelled.join(", ")
                ))
            }
        } else {
            Ok(())
        }
    }

    /// Write collected words to the words file
    fn flush_words_to_file(&self) -> Result<()> {
        let words_to_add = self.words_to_add.lock();
        if words_to_add.is_empty() {
            return Ok(());
        }

        let words_path = Path::new(&self.config.words_file);

        // Read existing words if file exists
        let mut all_words: HashSet<String> = if words_path.exists() {
            Self::load_custom_words(words_path).unwrap_or_default()
        } else {
            HashSet::new()
        };

        // Add new words
        let new_count = words_to_add.iter().filter(|w| !all_words.contains(*w)).count();
        for word in words_to_add.iter() {
            all_words.insert(word.clone());
        }

        if new_count == 0 {
            return Ok(());
        }

        // Sort and write
        let mut sorted: Vec<_> = all_words.into_iter().collect();
        sorted.sort();

        let mut file = fs::File::create(words_path)
            .with_context(|| format!("Failed to create words file: {}", words_path.display()))?;
        for word in &sorted {
            writeln!(file, "{}", word)?;
        }

        println!("Added {} word(s) to {}", new_count, words_path.display());
        Ok(())
    }
}

impl ProductDiscovery for SpellcheckProcessor {
    fn description(&self) -> &str {
        "Check documentation files for spelling errors"
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        !file_index.scan(&self.config.scan, true).is_empty()
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex) -> Result<()> {
        let mut extra_inputs = self.config.extra_inputs.clone();
        for ai in &self.config.auto_inputs {
            extra_inputs.extend(config_file_inputs(ai));
        }
        // If custom words are enabled, add the words file as an input so
        // changes to it invalidate all spellcheck products.
        if self.config.use_words_file {
            let words_path = Path::new(&self.config.words_file);
            if words_path.exists() {
                extra_inputs.push(self.config.words_file.clone());
            }
        }
        discover_checker_products(
            graph,
            &self.config.scan,
            file_index,
            &extra_inputs,
            &self.config,
            crate::processors::names::SPELLCHECK,
        )
    }

    fn execute(&self, product: &Product) -> Result<()> {
        let result = self.check_file(product.primary_input());
        // In auto_add_words mode, flush after each file when not batching.
        // Ignore flush errors so they don't mask the actual check result.
        if self.config.auto_add_words
            && let Err(e) = self.flush_words_to_file()
        {
            eprintln!("Warning: failed to flush spellcheck words file: {}", e);
        }
        result
    }

    fn config_json(&self) -> Option<String> {
        serde_json::to_string(&self.config).ok()
    }

    fn supports_batch(&self) -> bool {
        self.config.auto_add_words
    }

    fn execute_batch(&self, products: &[&Product]) -> Vec<Result<()>> {
        let results: Vec<Result<()>> = products
            .iter()
            .map(|p| self.check_file(p.primary_input()))
            .collect();

        // Flush all collected words at the end of the batch
        if self.config.auto_add_words
            && let Err(e) = self.flush_words_to_file() {
                eprintln!("Warning: failed to flush spellcheck words file: {}", e);
            }

        results
    }
}
