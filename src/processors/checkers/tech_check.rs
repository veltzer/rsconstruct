use anyhow::{Result, bail};
use regex::Regex;
use std::collections::HashSet;
use std::fs;
use std::path::Path;

use crate::config::TechCheckConfig;
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{discover_checker_products, execute_checker_batch};

pub struct TechCheckProcessor {
    config: TechCheckConfig,
}

impl TechCheckProcessor {
    pub fn new(config: TechCheckConfig) -> Self {
        Self { config }
    }

    fn execute_product(&self, product: &Product) -> Result<()> {
        self.check_files(&[product.primary_input()])
    }

    fn check_files(&self, files: &[&Path]) -> Result<()> {
        let terms = load_terms(&self.config.tech_files_dir)?;
        if terms.is_empty() {
            return Ok(());
        }
        let pattern = create_pattern(&terms);
        let mut errors = Vec::new();
        let mut all_backticked = HashSet::new();

        for file in files {
            let content = fs::read_to_string(file)?;
            let unquoted = find_unquoted_terms(&content, &pattern);
            for (line_num, term) in &unquoted {
                errors.push(format!(
                    "{}:{}: tech term `{}` is not backtick-quoted",
                    file.display(), line_num, term,
                ));
            }
            let backticked = find_backticked_terms(&content);
            for t in &backticked {
                let is_known = terms.iter().any(|term| term.eq_ignore_ascii_case(t));
                if !is_known {
                    errors.push(format!(
                        "{}: `{}` is backtick-quoted but not in tech term list",
                        file.display(), t,
                    ));
                }
            }
            all_backticked.extend(backticked);
        }

        // Check for unused terms (terms in the list but never backticked in any file)
        for term in &terms {
            let found = all_backticked.iter().any(|b| b.eq_ignore_ascii_case(term));
            if !found {
                errors.push(format!(
                    "tech term `{}` is in {} but never backtick-quoted in any file",
                    term, self.config.tech_files_dir,
                ));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            bail!("Tech check failures:\n{}", errors.join("\n"))
        }
    }
}

impl crate::processors::ProductDiscovery for TechCheckProcessor {
    fn description(&self) -> &str {
        "Check that technical terms are backtick-quoted in markdown files"
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        Path::new(&self.config.tech_files_dir).is_dir()
            && !file_index.scan(&self.config.scan, true).is_empty()
    }

    fn discover(
        &self,
        graph: &mut BuildGraph,
        file_index: &FileIndex,
    ) -> Result<()> {
        if !Path::new(&self.config.tech_files_dir).is_dir() {
            return Ok(());
        }
        // Collect all .txt files from tech_files_dir as extra inputs
        let mut extra_inputs = self.config.extra_inputs.clone();
        for entry in fs::read_dir(&self.config.tech_files_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "txt") {
                extra_inputs.push(path.to_string_lossy().into_owned());
            }
        }
        for ai in &self.config.auto_inputs {
            extra_inputs.extend(crate::processors::config_file_inputs(ai));
        }
        discover_checker_products(
            graph, &self.config.scan, file_index, &extra_inputs, &self.config,
            crate::processors::names::TECH_CHECK,
        )
    }

    fn execute(&self, product: &Product) -> Result<()> {
        self.execute_product(product)
    }

    fn supports_batch(&self) -> bool {
        self.config.batch
    }

    fn execute_batch(&self, products: &[&Product]) -> Vec<Result<()>> {
        execute_checker_batch(products, |files| self.check_files(files))
    }

    fn config_json(&self) -> Option<String> {
        serde_json::to_string(&self.config).ok()
    }
}

// --- Shared logic used by both the processor and the `tech fix` command ---

/// Load all technical terms from .txt files in the given directory.
/// Each file has one term per line.
pub fn load_terms(tech_files_dir: &str) -> Result<HashSet<String>> {
    let dir = Path::new(tech_files_dir);
    if !dir.is_dir() {
        bail!("tech_files_dir `{}` does not exist or is not a directory", tech_files_dir);
    }
    let mut terms = HashSet::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "txt") {
            let content = fs::read_to_string(&path)?;
            for line in content.lines() {
                let line = line.trim();
                if !line.is_empty() {
                    terms.insert(line.to_string());
                }
            }
        }
    }
    Ok(terms)
}

/// Build a regex pattern matching all terms, sorted longest-first
/// to avoid partial matches. Uses word boundaries and is case-insensitive.
pub fn create_pattern(terms: &HashSet<String>) -> Regex {
    let mut sorted: Vec<&str> = terms.iter().map(|s| s.as_str()).collect();
    sorted.sort_by_key(|b| std::cmp::Reverse(b.len()));
    let escaped: Vec<String> = sorted.iter().map(|t| regex::escape(t)).collect();
    let pattern = format!(r"\b(?:{})\b", escaped.join("|"));
    Regex::new(&format!("(?i){}", pattern)).expect("failed to compile tech terms regex")
}

/// Find ranges in the text that are inside fenced code blocks (``` ... ```)
/// Returns a sorted list of (start, end) byte ranges.
fn fenced_code_ranges(text: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut pos = 0;
    let bytes = text.as_bytes();
    while pos < bytes.len() {
        if bytes[pos] == b'`' && pos + 2 < bytes.len() && bytes[pos + 1] == b'`' && bytes[pos + 2] == b'`' {
            let start = pos;
            // Skip opening fence (may have more than 3 backticks and info string)
            pos += 3;
            while pos < bytes.len() && bytes[pos] == b'`' {
                pos += 1;
            }
            // Skip to end of opening fence line
            while pos < bytes.len() && bytes[pos] != b'\n' {
                pos += 1;
            }
            // Find closing fence
            loop {
                if pos >= bytes.len() {
                    ranges.push((start, bytes.len()));
                    break;
                }
                if bytes[pos] == b'\n' || pos == 0 {
                    let line_start = if bytes[pos] == b'\n' { pos + 1 } else { pos };
                    if line_start + 2 < bytes.len()
                        && bytes[line_start] == b'`'
                        && bytes[line_start + 1] == b'`'
                        && bytes[line_start + 2] == b'`'
                    {
                        // Skip to end of closing fence line
                        let mut end = line_start + 3;
                        while end < bytes.len() && bytes[end] != b'\n' {
                            end += 1;
                        }
                        if end < bytes.len() {
                            end += 1; // include the newline
                        }
                        ranges.push((start, end));
                        pos = end;
                        break;
                    }
                }
                pos += 1;
            }
        } else {
            pos += 1;
        }
    }
    ranges
}

/// Check if a byte position is inside a fenced code block or inline code span.
fn is_inside_code(text: &str, match_start: usize, match_end: usize, fenced_ranges: &[(usize, usize)]) -> bool {
    // Check fenced code blocks
    for &(start, end) in fenced_ranges {
        if match_start >= start && match_end <= end {
            return true;
        }
    }
    // Check inline code: count single backticks before position (outside fenced blocks)
    let before = &text[..match_start];
    let mut backtick_count = 0;
    let mut i = 0;
    let b = before.as_bytes();
    while i < b.len() {
        // Skip fenced ranges
        let mut in_fenced = false;
        for &(start, end) in fenced_ranges {
            if i >= start && i < end {
                i = end;
                in_fenced = true;
                break;
            }
        }
        if in_fenced {
            continue;
        }
        if i < b.len() && b[i] == b'`' {
            backtick_count += 1;
        }
        i += 1;
    }
    // Odd count means we're inside inline code
    backtick_count % 2 == 1
}

/// Check if a match is already wrapped in backticks.
fn is_already_backticked(text: &str, match_start: usize, match_end: usize) -> bool {
    match_start > 0
        && match_end < text.len()
        && text.as_bytes()[match_start - 1] == b'`'
        && text.as_bytes()[match_end] == b'`'
}

/// Find unquoted technical terms in a markdown file's content.
/// Returns (line_number, term) pairs for each unquoted occurrence.
pub fn find_unquoted_terms(content: &str, pattern: &Regex) -> Vec<(usize, String)> {
    let fenced = fenced_code_ranges(content);
    let mut results = Vec::new();
    for m in pattern.find_iter(content) {
        let start = m.start();
        let end = m.end();
        if is_already_backticked(content, start, end) {
            continue;
        }
        if is_inside_code(content, start, end, &fenced) {
            continue;
        }
        // Calculate line number
        let line_num = content[..start].matches('\n').count() + 1;
        results.push((line_num, m.as_str().to_string()));
    }
    results
}

/// Split a backticked string into individual terms.
/// Handles comma-separated lists like "sed, awk" and separators like "and", "or", "/".
fn split_backticked(inner: &str) -> Vec<String> {
    // Split on comma, " and ", " or ", "/"
    let parts: Vec<&str> = inner.split(',').collect();
    let mut results = Vec::new();
    for part in parts {
        // Further split on " and " / " or " / "/"
        for sub in part.split('/') {
            for tok in sub.split(" and ").flat_map(|s| s.split(" or ")) {
                let trimmed = tok.trim();
                if !trimmed.is_empty() {
                    results.push(trimmed.to_string());
                }
            }
        }
    }
    results
}

/// Extract all backtick-quoted terms from a markdown file's content,
/// excluding content inside fenced code blocks.
/// Handles grouped terms like `` `sed, awk` `` by splitting them.
pub fn find_backticked_terms(content: &str) -> HashSet<String> {
    let fenced = fenced_code_ranges(content);
    let backtick_re = Regex::new(r"`([^`]+)`").expect("backtick regex");
    let mut terms = HashSet::new();
    for m in backtick_re.find_iter(content) {
        let start = m.start();
        let end = m.end();
        // Skip if inside fenced code block
        let mut in_fenced = false;
        for &(fs, fe) in &fenced {
            if start >= fs && end <= fe {
                in_fenced = true;
                break;
            }
        }
        if in_fenced {
            continue;
        }
        // Extract the content between backticks and split grouped terms
        // Skip anything that looks like inline code rather than a term reference
        let inner = &content[start + 1..end - 1];
        if !looks_like_term_reference(inner) {
            continue;
        }
        for term in split_backticked(inner) {
            terms.insert(term);
        }
    }
    terms
}

/// Find unquoted term positions (byte offsets) for the fix command.
/// Returns (start, end, matched_text) sorted by position.
fn find_unquoted_positions(content: &str, pattern: &Regex) -> Vec<(usize, usize, String)> {
    let fenced = fenced_code_ranges(content);
    let mut results = Vec::new();
    for m in pattern.find_iter(content) {
        let start = m.start();
        let end = m.end();
        if is_already_backticked(content, start, end) {
            continue;
        }
        if is_inside_code(content, start, end, &fenced) {
            continue;
        }
        results.push((start, end, m.as_str().to_string()));
    }
    results
}

/// Check if a backticked string looks like a term reference (not arbitrary inline code).
/// Code snippets like `ls -la`, `pip install foo`, `x = 5` should keep their backticks.
fn looks_like_term_reference(inner: &str) -> bool {
    let parts = split_backticked(inner);
    // Must have at least one part
    if parts.is_empty() {
        return false;
    }
    // Each part should look like a plausible term name:
    // no shell/code characters that indicate it's a code snippet
    let code_chars = ['(', ')', '{', '}', '[', ']', ';', '=', '>', '<', '|', '\\', '"', '\''];
    for part in &parts {
        if part.contains(' ') || part.chars().any(|c| code_chars.contains(&c)) {
            return false;
        }
    }
    true
}

/// Find backtick-quoted terms that are NOT in the tech term list.
/// Returns (start, end) byte positions of the full `term` span (including backticks).
/// Only considers spans that look like term references, not arbitrary inline code.
fn find_non_tech_backticked_positions(content: &str, terms: &HashSet<String>) -> Vec<(usize, usize)> {
    let fenced = fenced_code_ranges(content);
    let backtick_re = Regex::new(r"`([^`]+)`").expect("backtick regex");
    let mut results = Vec::new();
    for m in backtick_re.find_iter(content) {
        let start = m.start();
        let end = m.end();
        let mut in_fenced = false;
        for &(fs, fe) in &fenced {
            if start >= fs && end <= fe {
                in_fenced = true;
                break;
            }
        }
        if in_fenced {
            continue;
        }
        let inner = &content[start + 1..end - 1];
        // Skip anything that looks like inline code rather than a term reference
        if !looks_like_term_reference(inner) {
            continue;
        }
        // Split grouped terms and check each part
        let parts = split_backticked(inner);
        let all_non_tech = parts.iter().all(|p| {
            !terms.iter().any(|t| t.eq_ignore_ascii_case(p))
        });
        // Only flag for removal if none of the parts are tech terms
        if all_non_tech {
            results.push((start, end));
        }
    }
    results
}

/// Auto-fix a single markdown file: add backticks to unquoted tech terms,
/// remove backticks from non-tech backticked terms.
/// Returns true if the file was modified.
pub fn fix_file(path: &Path, terms: &HashSet<String>, pattern: &Regex) -> Result<bool> {
    let content = fs::read_to_string(path)?;

    // Collect all edits as (start, end, replacement)
    let mut edits: Vec<(usize, usize, String)> = Vec::new();

    // Add backticks to unquoted terms
    for (start, end, matched) in find_unquoted_positions(&content, pattern) {
        edits.push((start, end, format!("`{}`", matched)));
    }

    // Remove backticks from non-tech terms
    for (start, end) in find_non_tech_backticked_positions(&content, terms) {
        let inner = &content[start + 1..end - 1];
        edits.push((start, end, inner.to_string()));
    }

    if edits.is_empty() {
        return Ok(false);
    }

    // Sort by position descending so we can apply from right to left
    edits.sort_by(|a, b| b.0.cmp(&a.0));

    // Remove overlapping edits (keep the first one, i.e., rightmost after sorting desc)
    edits.dedup_by(|a, b| {
        // a comes after b in position (since sorted desc)
        // They overlap if a.start < b.end
        a.0 < b.1
    });

    let mut result = content.clone();
    for (start, end, replacement) in &edits {
        result = format!("{}{}{}", &result[..*start], replacement, &result[*end..]);
    }

    if result != content {
        fs::write(path, &result)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Fix all markdown files: called by `rsconstruct tech fix`.
/// Uses the same scan config as the tech_check processor to find files.
pub fn fix_all(config: &TechCheckConfig) -> Result<()> {
    let terms = load_terms(&config.tech_files_dir)?;
    if terms.is_empty() {
        println!("No technical terms found in {}", config.tech_files_dir);
        return Ok(());
    }
    let pattern = create_pattern(&terms);

    let file_index = FileIndex::build()?;
    let md_files = file_index.scan(&config.scan, true);

    if md_files.is_empty() {
        println!("No markdown files found");
        return Ok(());
    }

    println!("Checking {} markdown files against {} tech terms...", md_files.len(), terms.len());

    let mut modified_count = 0;
    for file in &md_files {
        if fix_file(file, &terms, &pattern)? {
            modified_count += 1;
            println!("  Fixed: {}", file.display());
        }
    }

    println!("Done. Modified {} of {} files.", modified_count, md_files.len());
    Ok(())
}
