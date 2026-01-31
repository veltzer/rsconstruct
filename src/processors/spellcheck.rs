use anyhow::{Context, Result};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::config::SpellcheckConfig;
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use super::{ProductDiscovery, discover_stub_products, ensure_stub_dir, write_stub, clean_outputs};

const SPELLCHECK_STUB_DIR: &str = "out/spellcheck";
const DICT_DIR: &str = "/usr/share/hunspell";

pub struct SpellcheckProcessor {
    project_root: PathBuf,
    spellcheck_config: SpellcheckConfig,
    stub_dir: PathBuf,
    /// Cached dictionary, built once on first use and reused across all execute() calls
    cached_dict: OnceLock<Result<zspell::Dictionary, String>>,
    /// Custom words, loaded once at initialization
    custom_words: HashSet<String>,
}

impl SpellcheckProcessor {
    pub fn new(project_root: PathBuf, spellcheck_config: SpellcheckConfig) -> Result<Self> {
        let stub_dir = project_root.join(SPELLCHECK_STUB_DIR);
        let custom_words = if spellcheck_config.use_words_file {
            let words_path = project_root.join(&spellcheck_config.words_file);
            Self::load_custom_words(&words_path)
                .with_context(|| format!("Custom words file not found: {}", words_path.display()))?
        } else {
            HashSet::new()
        };
        Ok(Self {
            project_root,
            spellcheck_config,
            stub_dir,
            cached_dict: OnceLock::new(),
            custom_words,
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
        let lang = &self.spellcheck_config.language;
        let aff_path = Path::new(DICT_DIR).join(format!("{}.aff", lang));
        let dic_path = Path::new(DICT_DIR).join(format!("{}.dic", lang));

        let aff_content = fs::read_to_string(&aff_path)
            .context(format!("Failed to read affix file: {}. Is the hunspell dictionary for '{}' installed?", aff_path.display(), lang))?;
        let dic_content = fs::read_to_string(&dic_path)
            .context(format!("Failed to read dictionary file: {}. Is the hunspell dictionary for '{}' installed?", dic_path.display(), lang))?;

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

    /// Strip markdown syntax from a line
    fn strip_markdown(line: &str) -> String {
        let mut result = line.to_string();

        // Remove inline code spans
        while let Some(start) = result.find('`') {
            if let Some(end) = result[start + 1..].find('`') {
                result = format!("{} {}", &result[..start], &result[start + 1 + end + 1..]);
            } else {
                break;
            }
        }

        // Remove URLs: [text](url) -> text
        while let Some(bracket_start) = result.find('[') {
            if let Some(bracket_end) = result[bracket_start..].find("](") {
                let abs_bracket_end = bracket_start + bracket_end;
                if let Some(paren_end) = result[abs_bracket_end + 2..].find(')') {
                    let link_text = &result[bracket_start + 1..abs_bracket_end];
                    result = format!("{}{}{}", &result[..bracket_start], link_text, &result[abs_bracket_end + 2 + paren_end + 1..]);
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        // Remove bare URLs (http/https)
        let url_patterns = ["https://", "http://"];
        for pattern in &url_patterns {
            while let Some(start) = result.find(pattern) {
                let end = result[start..].find(|c: char| c.is_whitespace() || c == ')' || c == '>' || c == '"')
                    .map(|e| start + e)
                    .unwrap_or(result.len());
                result = format!("{} {}", &result[..start], &result[end..]);
            }
        }

        // Remove HTML tags
        while let Some(start) = result.find('<') {
            if let Some(end) = result[start..].find('>') {
                result = format!("{} {}", &result[..start], &result[start + end + 1..]);
            } else {
                break;
            }
        }

        result
    }

    /// Check a single file for spelling errors
    fn check_file(&self, doc_file: &Path, stub_path: &Path, dict: &zspell::Dictionary, custom_words: &HashSet<String>) -> Result<()> {
        let content = fs::read_to_string(doc_file)
            .context(format!("Failed to read document file: {}", doc_file.display()))?;

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
            misspelled.sort();
            return Err(anyhow::anyhow!(
                "Spelling errors in {}:\n  {}",
                doc_file.display(),
                misspelled.join(", ")
            ));
        }

        write_stub(stub_path, "spellchecked")
    }
}

impl ProductDiscovery for SpellcheckProcessor {
    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        !file_index.scan(&self.project_root, &self.spellcheck_config.scan, true).is_empty()
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex) -> Result<()> {
        discover_stub_products(
            graph,
            &self.project_root,
            &self.stub_dir,
            &self.spellcheck_config.scan,
            file_index,
            &self.spellcheck_config.extra_inputs,
            &self.spellcheck_config,
            "spellcheck",
            "spellcheck",
            true,
        )
    }

    fn execute(&self, product: &Product) -> Result<()> {
        if product.outputs.len() != 1 {
            anyhow::bail!("Spellcheck product must have exactly one output");
        }

        ensure_stub_dir(&self.stub_dir, "spellcheck")?;

        let dict = self.get_dictionary()?;
        let custom_words = &self.custom_words;

        self.check_file(&product.inputs[0], &product.outputs[0], dict, custom_words)
    }

    fn clean(&self, product: &Product) -> Result<()> {
        clean_outputs(product, "spellcheck")
    }
}
