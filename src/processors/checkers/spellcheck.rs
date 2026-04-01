use anyhow::{Context, Result};
use regex::Regex;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::sync::OnceLock;

use crate::config::SpellcheckConfig;
use crate::errors;
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProductDiscovery, config_file_inputs, discover_checker_products};
use crate::processors::word_manager::WordManager;

const DICT_DIR: &str = "/usr/share/hunspell";

pub struct SpellcheckProcessor {
    config: SpellcheckConfig,
    /// Cached dictionary, built once on first use and reused across all execute() calls
    cached_dict: OnceLock<Result<zspell::Dictionary, String>>,
    words: WordManager,
}

impl SpellcheckProcessor {
    pub fn new(config: SpellcheckConfig) -> Self {
        let words_path = Path::new(&config.words_file);
        let custom_words = if words_path.exists() {
            Self::load_custom_words(words_path).unwrap_or_default()
        } else {
            HashSet::new()
        };
        let words = WordManager::new(
            custom_words,
            config.words_file.clone(),
            None,
        );
        Self {
            config,
            cached_dict: OnceLock::new(),
            words,
        }
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

            if self.words.is_known(&lower) {
                continue;
            }

            if !dict.check_word(&lower) {
                misspelled.push(word.clone());
            }
        }

        misspelled.sort();
        self.words.handle_misspelled(&misspelled, doc_file, self.config.auto_add_words)
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
        self.words.execute_with_flush(
            product,
            self.config.auto_add_words,
            |file| self.check_file(file),
            "spellcheck",
        )
    }

    fn config_json(&self) -> Option<String> {
        serde_json::to_string(&self.config).ok()
    }

    fn supports_batch(&self) -> bool {
        self.config.auto_add_words
    }

    fn execute_batch(&self, products: &[&Product]) -> Vec<Result<()>> {
        self.words.execute_batch_with_flush(
            products,
            self.config.auto_add_words,
            |file| self.check_file(file),
            "spellcheck",
        )
    }
}
