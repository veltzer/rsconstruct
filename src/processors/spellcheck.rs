use anyhow::{Context, Result};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use walkdir::WalkDir;

use crate::config::{SpellcheckConfig, config_hash};
use crate::graph::{BuildGraph, Product};
use crate::ignore::IgnoreRules;
use super::ProductDiscovery;

const SPELLCHECK_STUB_DIR: &str = "out/spellcheck";
const DICT_DIR: &str = "/usr/share/hunspell";

pub struct SpellcheckProcessor {
    project_root: PathBuf,
    spellcheck_config: SpellcheckConfig,
    stub_dir: PathBuf,
    ignore_rules: Arc<IgnoreRules>,
}

impl SpellcheckProcessor {
    pub fn new(project_root: PathBuf, spellcheck_config: SpellcheckConfig, ignore_rules: Arc<IgnoreRules>) -> Self {
        let stub_dir = project_root.join(SPELLCHECK_STUB_DIR);
        Self {
            project_root,
            spellcheck_config,
            stub_dir,
            ignore_rules,
        }
    }

    /// Check if any matching doc files exist
    fn should_check(&self) -> bool {
        !self.find_doc_files().is_empty()
    }

    /// Find all document files matching configured extensions
    fn find_doc_files(&self) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = WalkDir::new(&self.project_root)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| {
                let path = e.path();

                // Skip common non-source directories
                let path_str = path.to_string_lossy();
                if path_str.contains("/.git/")
                    || path_str.contains("/out/")
                    || path_str.contains("/.rsb/")
                    || path_str.contains("/node_modules/")
                    || path_str.contains("/build/")
                    || path_str.contains("/dist/")
                    || path_str.contains("/target/")
                {
                    return false;
                }

                // Check if file matches any configured extension
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                self.spellcheck_config.extensions.iter().any(|ext| name.ends_with(ext.as_str()))
            })
            .map(|e| e.path().to_path_buf())
            .filter(|p| !self.ignore_rules.is_ignored(p))
            .collect();
        files.sort();
        files
    }

    /// Get stub path for a document file
    fn get_stub_path(&self, doc_file: &Path) -> PathBuf {
        let relative_path = doc_file
            .strip_prefix(&self.project_root)
            .unwrap_or(doc_file);
        let stub_name = format!(
            "{}.spellcheck",
            relative_path.display().to_string().replace(['/', '\\'], "_")
        );
        self.stub_dir.join(stub_name)
    }

    /// Path to the custom words file
    fn words_file_path(&self) -> PathBuf {
        self.project_root.join(&self.spellcheck_config.words_file)
    }

    /// Load custom words from the words file
    fn load_custom_words(&self) -> HashSet<String> {
        let words_path = self.words_file_path();
        let mut words = HashSet::new();
        if let Ok(content) = fs::read_to_string(&words_path) {
            for line in content.lines() {
                let trimmed = line.trim();
                if !trimmed.is_empty() && !trimmed.starts_with('#') {
                    words.insert(trimmed.to_lowercase());
                }
            }
        }
        words
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

        // Create stub file on success
        if let Some(parent) = stub_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(stub_path, "spellchecked").context("Failed to create spellcheck stub file")?;

        Ok(())
    }
}

impl ProductDiscovery for SpellcheckProcessor {
    fn discover(&self, graph: &mut BuildGraph) -> Result<()> {
        if !self.should_check() {
            return Ok(());
        }

        let doc_files = self.find_doc_files();
        let config_hash = Some(config_hash(&self.spellcheck_config));
        let words_file = self.words_file_path();
        let words_file_exists = words_file.exists();

        for doc_file in doc_files {
            let stub_path = self.get_stub_path(&doc_file);
            let mut inputs = vec![doc_file];
            if words_file_exists {
                inputs.push(words_file.clone());
            }
            graph.add_product(
                inputs,
                vec![stub_path],
                "spellcheck",
                config_hash.clone(),
            );
        }

        Ok(())
    }

    fn execute(&self, product: &Product) -> Result<()> {
        if product.outputs.len() != 1 {
            anyhow::bail!("Spellcheck product must have exactly one output");
        }

        // Ensure stub directory exists
        if !self.stub_dir.exists() {
            fs::create_dir_all(&self.stub_dir)
                .context("Failed to create spellcheck stub directory")?;
        }

        let dict = self.build_dictionary()?;
        let custom_words = self.load_custom_words();

        // First input is always the doc file
        self.check_file(&product.inputs[0], &product.outputs[0], &dict, &custom_words)
    }

    fn clean(&self, product: &Product) -> Result<()> {
        for output in &product.outputs {
            if output.exists() {
                fs::remove_file(output)?;
                println!("Removed spellcheck stub: {}", output.display());
            }
        }
        Ok(())
    }
}
