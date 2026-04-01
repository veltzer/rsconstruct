use anyhow::{Result, bail};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

use crate::config::TermsConfig;
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{discover_checker_products, execute_checker_batch};

pub struct TermsProcessor {
    config: TermsConfig,
}

impl TermsProcessor {
    pub fn new(config: TermsConfig) -> Self {
        Self { config }
    }

    fn execute_product(&self, product: &Product) -> Result<()> {
        self.check_files(&[product.primary_input()])
    }

    fn check_files(&self, files: &[&Path]) -> Result<()> {
        let terms = load_terms(&self.config.terms_dir)?;
        if terms.is_empty() {
            return Ok(());
        }
        let sorted = sorted_terms(&terms);
        let mut bad_files = Vec::new();

        for file in files {
            if check_file(file, &terms, &sorted)? {
                bad_files.push(file.display().to_string());
            }
        }

        if bad_files.is_empty() {
            Ok(())
        } else {
            bail!(
                "{} file(s) have term issues (run `rsconstruct terms fix` to fix):\n{}",
                bad_files.len(),
                bad_files.join("\n"),
            )
        }
    }
}

impl crate::processors::ProductDiscovery for TermsProcessor {
    fn description(&self) -> &str {
        "Check that technical terms are backtick-quoted in markdown files"
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        Path::new(&self.config.terms_dir).is_dir()
            && !file_index.scan(&self.config.scan, true).is_empty()
    }

    fn discover(
        &self,
        graph: &mut BuildGraph,
        file_index: &FileIndex,
    ) -> Result<()> {
        if !Path::new(&self.config.terms_dir).is_dir() {
            return Ok(());
        }
        // Collect all .txt files from terms_dir as extra inputs
        let mut extra_inputs = self.config.extra_inputs.clone();
        for entry in fs::read_dir(&self.config.terms_dir)? {
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
            crate::processors::names::TERMS,
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

// --- Shared logic used by both the processor and the `rsconstruct terms fix` command ---

/// Load all technical terms from .txt files in the given directory.
/// Each file has one term per line. Errors if any term appears more than once
/// (within the same file or across files).
pub fn load_terms(terms_dir: &str) -> Result<HashSet<String>> {
    let dir = Path::new(terms_dir);
    if !dir.is_dir() {
        bail!("terms_dir `{}` does not exist or is not a directory", terms_dir);
    }
    // Map each term to (file, line_number) where it first appeared
    let mut seen: std::collections::HashMap<String, (String, usize)> = std::collections::HashMap::new();
    let mut duplicates = Vec::new();

    let mut entries: Vec<_> = fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.path());

    for entry in &entries {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "txt") {
            let filename = path.file_name().unwrap().to_string_lossy().to_string();
            let content = fs::read_to_string(&path)?;
            for (line_idx, line) in content.lines().enumerate() {
                let term = line.trim();
                if term.is_empty() {
                    continue;
                }
                if let Some((prev_file, prev_line)) = seen.get(term) {
                    duplicates.push(format!(
                        "  `{}` in {}:{} (first seen in {}:{})",
                        term, filename, line_idx + 1, prev_file, prev_line,
                    ));
                } else {
                    seen.insert(term.to_string(), (filename.clone(), line_idx + 1));
                }
            }
        }
    }

    if !duplicates.is_empty() {
        bail!("Duplicate terms in {}:\n{}", terms_dir, duplicates.join("\n"));
    }

    Ok(seen.into_keys().collect())
}

/// Sort terms longest-first for greedy matching (so "Android Studio" matches before "Android").
fn sorted_terms(terms: &HashSet<String>) -> Vec<&str> {
    let mut sorted: Vec<&str> = terms.iter().map(|s| s.as_str()).collect();
    sorted.sort_by_key(|b| std::cmp::Reverse(b.len()));
    sorted
}

// --- Text analysis helpers ---

/// Find ranges in the text that should be excluded from term processing:
/// YAML frontmatter (--- ... ---) and fenced code blocks (``` ... ```).
fn excluded_ranges(text: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();

    // YAML frontmatter: must start at the very beginning of the file
    if text.starts_with("---\n") || text.starts_with("---\r\n") {
        let skip = if text.as_bytes().get(3) == Some(&b'\r') { 5 } else { 4 };
        if let Some(end_idx) = text[skip..].find("\n---") {
            let mut end = skip + end_idx + 4; // past the closing ---
            // Skip to end of line
            while end < text.len() && text.as_bytes()[end] != b'\n' {
                end += 1;
            }
            if end < text.len() {
                end += 1;
            }
            ranges.push((0, end));
        }
    }

    // Fenced code blocks
    let mut pos = 0;
    let bytes = text.as_bytes();
    while pos < bytes.len() {
        if bytes[pos] == b'`' && pos + 2 < bytes.len() && bytes[pos + 1] == b'`' && bytes[pos + 2] == b'`' {
            let start = pos;
            pos += 3;
            while pos < bytes.len() && bytes[pos] == b'`' {
                pos += 1;
            }
            while pos < bytes.len() && bytes[pos] != b'\n' {
                pos += 1;
            }
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
                        let mut end = line_start + 3;
                        while end < bytes.len() && bytes[end] != b'\n' {
                            end += 1;
                        }
                        if end < bytes.len() {
                            end += 1;
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

/// Find all backtick span ranges (start, end) in text, excluding fenced code blocks.
/// Returns positions of the opening and closing backtick (inclusive of backticks).
fn backtick_span_ranges(text: &str, fenced: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // Skip fenced code blocks
        let mut in_fenced = false;
        for &(fs, fe) in fenced {
            if i >= fs && i < fe {
                i = fe;
                in_fenced = true;
                break;
            }
        }
        if in_fenced {
            continue;
        }
        if bytes[i] == b'`' {
            // Find matching closing backtick
            let open = i;
            i += 1;
            while i < bytes.len() && bytes[i] != b'`' && bytes[i] != b'\n' {
                i += 1;
            }
            if i < bytes.len() && bytes[i] == b'`' && i > open {
                spans.push((open, i + 1)); // include both backticks
            }
            if i < bytes.len() {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    spans
}

/// Check if a byte position is inside any of the given ranges.
fn inside_ranges(pos: usize, end: usize, ranges: &[(usize, usize)]) -> bool {
    ranges.iter().any(|&(s, e)| pos >= s && end <= e)
}

/// Check if the character at a byte position is a word-boundary character.
/// A term match is valid if the characters immediately before and after it
/// are not alphanumeric (or the match is at the start/end of text).
fn is_word_boundary(text: &[u8], pos: usize) -> bool {
    if pos >= text.len() {
        return true;
    }
    let ch = text[pos];
    // Not a word character: anything that's not alphanumeric or underscore
    !ch.is_ascii_alphanumeric() && ch != b'_'
}

/// Case-insensitive substring search. Returns byte offset of the match, or None.
fn find_case_insensitive(haystack: &str, needle: &str, start: usize) -> Option<usize> {
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() || start + n.len() > h.len() {
        return None;
    }
    let mut i = start;
    let limit = h.len() - n.len();
    while i <= limit {
        if h[i..i + n.len()].eq_ignore_ascii_case(n) {
            return Some(i);
        }
        // Advance to next UTF-8 char boundary
        i += 1;
        while i <= limit && !haystack.is_char_boundary(i) {
            i += 1;
        }
    }
    None
}

/// Find all occurrences of a term in text (case-insensitive, word-boundary).
/// Returns (start, end) byte positions for each match.
fn find_term_occurrences(text: &str, term: &str) -> Vec<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut results = Vec::new();
    let mut pos = 0;
    while let Some(start) = find_case_insensitive(text, term, pos) {
        let end = start + term.len();
        // Check word boundaries
        let before_ok = start == 0 || is_word_boundary(bytes, start - 1);
        let after_ok = end >= bytes.len() || is_word_boundary(bytes, end);
        if before_ok && after_ok {
            results.push((start, end));
            pos = end;
        } else {
            // Advance to next UTF-8 char boundary
            pos = start + 1;
            while pos < text.len() && !text.is_char_boundary(pos) {
                pos += 1;
            }
        }
    }
    results
}

// --- Core check/fix logic ---

/// Split a backticked string into individual terms.
/// Handles comma-separated lists like "sed, awk" and word separators "and"/"or".
fn split_backticked(inner: &str) -> Vec<String> {
    let mut results = Vec::new();
    for part in inner.split(',') {
        for tok in part.split(" and ").flat_map(|s| s.split(" or ")) {
            let trimmed = tok.trim();
            if !trimmed.is_empty() {
                results.push(trimmed.to_string());
            }
        }
    }
    results
}

/// Check if a string with `/` looks like a file path rather than a term like CI/CD.
/// File paths have dots (extensions) or multiple segments.
fn is_file_path(s: &str) -> bool {
    if !s.contains('/') {
        return false;
    }
    // Has a dot after a slash → likely a file path (e.g., doc/ai.txt)
    if let Some(last_slash) = s.rfind('/')
        && s[last_slash..].contains('.')
    {
        return true;
    }
    // More than one slash → likely a file path (e.g., syllabi/courses/ai)
    s.matches('/').count() > 1
}

/// Check if a backticked string looks like a term reference (not arbitrary inline code
/// or a file path). Code snippets, file paths, and shell commands should keep their backticks.
fn looks_like_term_reference(inner: &str) -> bool {
    let parts = split_backticked(inner);
    if parts.is_empty() {
        return false;
    }
    if is_file_path(inner) {
        return false;
    }
    let code_chars = ['(', ')', '{', '}', '[', ']', ';', '=', '>', '<', '|', '\\', '"', '\'', '~'];
    for part in &parts {
        if part.contains(' ') || part.chars().any(|c| code_chars.contains(&c)) {
            return false;
        }
    }
    true
}

/// Find unquoted term positions (byte offsets) for the fix command.
/// Returns (start, end, term_text) sorted longest-first, non-overlapping.
fn find_unquoted_positions(content: &str, sorted_terms: &[&str]) -> Vec<(usize, usize, String)> {
    let fenced = excluded_ranges(content);
    let backtick_spans = backtick_span_ranges(content, &fenced);
    let mut claimed: Vec<(usize, usize)> = Vec::new();
    let mut results = Vec::new();

    for &term in sorted_terms {
        for (start, end) in find_term_occurrences(content, term) {
            if inside_ranges(start, end, &fenced) {
                continue;
            }
            if inside_ranges(start, end, &backtick_spans) {
                continue;
            }
            if claimed.iter().any(|&(cs, ce)| start < ce && end > cs) {
                continue;
            }
            claimed.push((start, end));
            results.push((start, end, content[start..end].to_string()));
        }
    }
    results
}

/// Find backtick-quoted terms that are NOT in the term list.
/// Only considers spans that look like term references, not arbitrary inline code.
fn find_non_tech_backticked_positions(content: &str, terms: &HashSet<String>) -> Vec<(usize, usize)> {
    let fenced = excluded_ranges(content);
    let spans = backtick_span_ranges(content, &fenced);
    let mut results = Vec::new();
    for &(start, end) in &spans {
        let inner = &content[start + 1..end - 1];
        if !looks_like_term_reference(inner) {
            continue;
        }
        let parts = split_backticked(inner);
        let all_non_tech = parts.iter().all(|p| {
            !terms.iter().any(|t| t.eq_ignore_ascii_case(p))
        });
        if all_non_tech {
            results.push((start, end));
        }
    }
    results
}

/// Apply edits to text, right-to-left. Edits must not overlap.
fn apply_edits(content: &str, edits: &mut Vec<(usize, usize, String)>) -> String {
    edits.sort_by(|a, b| b.0.cmp(&a.0));
    edits.dedup_by(|a, b| a.1 > b.0);

    let mut result = content.to_string();
    for (start, end, replacement) in edits.iter() {
        result = format!("{}{}{}", &result[..*start], replacement, &result[*end..]);
    }
    result
}

/// Apply term fixes to content: optionally remove non-tech backticks, then add missing backticks.
/// When `remove_non_terms` is true, backticks around non-terms are removed first.
/// Returns the fixed content.
fn fix_content(original: &str, terms: &HashSet<String>, sorted_terms: &[&str], remove_non_terms: bool) -> String {
    // Step 1: optionally remove backticks from non-terms (e.g. `CI`/`CD` → CI/CD)
    let cleaned = if remove_non_terms {
        let mut removals: Vec<(usize, usize, String)> = find_non_tech_backticked_positions(original, terms)
            .into_iter()
            .map(|(s, e)| (s, e, original[s + 1..e - 1].to_string()))
            .collect();
        if removals.is_empty() {
            original.to_string()
        } else {
            apply_edits(original, &mut removals)
        }
    } else {
        original.to_string()
    };

    // Step 2: add backticks to unquoted terms (on the cleaned text, so CI/CD is now found)
    let mut additions: Vec<(usize, usize, String)> = find_unquoted_positions(&cleaned, sorted_terms)
        .into_iter()
        .map(|(s, e, m)| (s, e, format!("`{}`", m)))
        .collect();
    if additions.is_empty() {
        cleaned
    } else {
        apply_edits(&cleaned, &mut additions)
    }
}

/// Check if a file would be changed by `terms fix`. Returns true if it needs fixing.
fn check_file(path: &Path, terms: &HashSet<String>, sorted_terms: &[&str]) -> Result<bool> {
    let original = fs::read_to_string(path)?;
    let fixed = fix_content(&original, terms, sorted_terms, false);
    Ok(fixed != original)
}

/// Auto-fix a single markdown file. Returns true if the file was modified.
pub fn fix_file(path: &Path, terms: &HashSet<String>, sorted_terms: &[&str], remove_non_terms: bool) -> Result<bool> {
    let original = fs::read_to_string(path)?;
    let fixed = fix_content(&original, terms, sorted_terms, remove_non_terms);
    if fixed != original {
        fs::write(path, &fixed)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Fix all markdown files: called by `rsconstruct terms fix`.
/// Uses the same scan config as the terms processor to find files.
pub fn fix_all(config: &TermsConfig, remove_non_terms: bool) -> Result<()> {
    let terms = load_terms(&config.terms_dir)?;
    if terms.is_empty() {
        println!("No technical terms found in {}", config.terms_dir);
        return Ok(());
    }
    let sorted = sorted_terms(&terms);

    let file_index = FileIndex::build()?;
    let md_files = file_index.scan(&config.scan, true);

    if md_files.is_empty() {
        println!("No markdown files found");
        return Ok(());
    }

    println!("Checking {} markdown files against {} terms...", md_files.len(), terms.len());

    let mut modified_count = 0;
    for file in &md_files {
        if fix_file(file, &terms, &sorted, remove_non_terms)? {
            modified_count += 1;
            println!("  Fixed: {}", file.display());
        }
    }

    println!("Done. Modified {} of {} files.", modified_count, md_files.len());
    Ok(())
}

/// Merge terms from another project's terms directory into the current one.
/// For each .txt file in `source_dir`:
///   - If a file with the same name exists in `terms_dir`, merge (union) and sort the terms.
///   - Otherwise, copy the file as-is.
pub fn merge_terms(config: &TermsConfig, source_dir: &str) -> Result<()> {
    let src = Path::new(source_dir);
    if !src.is_dir() {
        bail!("Source directory `{}` does not exist or is not a directory", source_dir);
    }
    let dest = Path::new(&config.terms_dir);
    if !dest.is_dir() {
        bail!("Terms directory `{}` does not exist or is not a directory", config.terms_dir);
    }

    let mut merged_count = 0;
    let mut copied_count = 0;

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "txt") {
            continue;
        }
        let filename = path.file_name().unwrap();
        let dest_path = dest.join(filename);

        let source_content = fs::read_to_string(&path)?;
        let source_terms: HashSet<String> = source_content
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();

        if dest_path.exists() {
            let dest_content = fs::read_to_string(&dest_path)?;
            let dest_terms: HashSet<String> = dest_content
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect();
            let mut all_terms = dest_terms.clone();
            all_terms.extend(source_terms.clone());
            let mut sorted: Vec<String> = all_terms.into_iter().collect();
            sorted.sort();
            let content = sorted.join("\n") + "\n";
            if content != source_content || content != dest_content {
                fs::write(&dest_path, &content)?;
                fs::write(&path, &content)?;
                merged_count += 1;
                let added_to_dest = sorted.len() - dest_terms.len();
                let added_to_src = sorted.len() - source_terms.len();
                println!("  Merged: {} (+{} to dest, +{} to source)",
                    filename.to_string_lossy(), added_to_dest, added_to_src);
            }
        } else {
            let mut sorted: Vec<String> = source_terms.into_iter().collect();
            sorted.sort();
            fs::write(&dest_path, sorted.join("\n") + "\n")?;
            copied_count += 1;
            println!("  Copied to dest: {}", filename.to_string_lossy());
        }
    }

    // Copy files that exist in destination but not in source back to source
    for entry in fs::read_dir(dest)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "txt") {
            continue;
        }
        let filename = path.file_name().unwrap();
        let src_path = src.join(filename);
        if !src_path.exists() {
            fs::copy(&path, &src_path)?;
            copied_count += 1;
            println!("  Copied to source: {}", filename.to_string_lossy());
        }
    }

    println!("Done. Merged {} file(s), copied {} new file(s).", merged_count, copied_count);
    Ok(())
}
