use anyhow::{Context, Result, bail};
use redb::{ReadableDatabase, ReadableTable, ReadableTableMetadata, TableDefinition};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::{TagsConfig, config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProcessorType, ProductDiscovery, clean_outputs, scan_root_valid};

const FRONTMATTER: TableDefinition<&str, &str> = TableDefinition::new("frontmatter");
const TAG_INDEX: TableDefinition<&str, &str> = TableDefinition::new("tag_index");

pub struct TagsProcessor {
    config: TagsConfig,
}

impl TagsProcessor {
    pub fn new(config: TagsConfig) -> Self {
        Self { config }
    }
}

impl ProductDiscovery for TagsProcessor {
    fn description(&self) -> &str {
        "Extract YAML frontmatter tags from markdown files into a searchable database"
    }

    fn processor_type(&self) -> ProcessorType {
        ProcessorType::Generator
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        scan_root_valid(&self.config.scan)
            && !file_index.scan(&self.config.scan, true).is_empty()
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex) -> Result<()> {
        let files = file_index.scan(&self.config.scan, true);
        if files.is_empty() {
            return Ok(());
        }

        let extra = resolve_extra_inputs(&self.config.extra_inputs)?;
        let mut inputs = Vec::with_capacity(files.len() + extra.len() + 1);
        inputs.extend(files);
        inputs.extend_from_slice(&extra);

        // If a tags file exists, add it as an input so edits trigger rebuild
        let tags_file_path = Path::new(&self.config.tags_file);
        if tags_file_path.exists() {
            inputs.push(tags_file_path.to_path_buf());
        }

        let output = PathBuf::from(&self.config.output);
        graph.add_product(
            inputs,
            vec![output],
            crate::processors::names::TAGS,
            Some(config_hash(&self.config)),
        )?;

        Ok(())
    }

    fn execute(&self, product: &Product) -> Result<()> {
        let output_path = product.outputs.first()
            .expect(crate::errors::EMPTY_PRODUCT_OUTPUTS);

        // Ensure output directory exists
        if let Some(parent) = output_path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("Failed to create tags output directory: {}", parent.display()))?;
            }
        }

        // Collect frontmatter from all input .md files
        let mut all_frontmatter: HashMap<String, serde_json::Value> = HashMap::new();
        let mut tag_to_files: HashMap<String, Vec<String>> = HashMap::new();

        for input in &product.inputs {
            let ext = input.extension().and_then(|e| e.to_str()).unwrap_or("");
            if ext != "md" {
                continue;
            }
            let content = fs::read_to_string(input)
                .with_context(|| format!("Failed to read {}", input.display()))?;

            if let Some(fm) = parse_frontmatter(&content) {
                let file_key = input.display().to_string();

                // Index all frontmatter fields:
                // - list fields: each item becomes a tag (e.g. "docker" from tags: [docker])
                // - scalar fields: indexed as "key=value" (e.g. "level=intermediate")
                if let Some(obj) = fm.as_object() {
                    for (key, value) in obj {
                        match value {
                            serde_json::Value::Array(items) => {
                                for item in items {
                                    if let Some(s) = item.as_str() {
                                        tag_to_files.entry(s.to_string())
                                            .or_default()
                                            .push(file_key.clone());
                                    }
                                }
                            }
                            serde_json::Value::String(s) => {
                                let tag = format!("{}={}", key, s);
                                tag_to_files.entry(tag)
                                    .or_default()
                                    .push(file_key.clone());
                            }
                            _ => {}
                        }
                    }
                }

                all_frontmatter.insert(file_key, fm);
            }
        }

        // Validate tags against allowed set if tags file exists
        let tags_file_path = Path::new(&self.config.tags_file);
        if tags_file_path.exists() {
            let allowed = load_tags_file(tags_file_path)?;

            let mut unknown: Vec<(String, Vec<String>)> = Vec::new();
            for (tag, files) in &tag_to_files {
                if !tag_matches_allowed(tag, &allowed) {
                    unknown.push((tag.clone(), files.clone()));
                }
            }
            if !unknown.is_empty() {
                unknown.sort_by(|a, b| a.0.cmp(&b.0));
                let mut msg = String::from("Unknown tags found (not in .tags file):\n");
                for (tag, files) in &unknown {
                    msg.push_str(&format!("  {}", tag));
                    if let Some(suggestion) = find_similar_tag(tag, &allowed) {
                        msg.push_str(&format!(" (did you mean '{}'?)", suggestion));
                    }
                    msg.push('\n');
                    for file in files {
                        msg.push_str(&format!("    - {}\n", file));
                    }
                }
                bail!("{}", msg.trim_end());
            }
        } else if self.config.tags_file_strict {
            bail!("Tags file not found: {}. Run 'rsb tags init' to create one.", self.config.tags_file);
        }

        // Write to redb database
        let db = crate::db::open_or_recreate(output_path, "tags database")?;

        let write_txn = db.begin_write()
            .context("Failed to begin write transaction")?;
        {
            let mut fm_table = write_txn.open_table(FRONTMATTER)
                .context("Failed to open frontmatter table")?;
            for (file, value) in &all_frontmatter {
                let json = serde_json::to_string(value).expect(crate::errors::JSON_SERIALIZE);
                fm_table.insert(file.as_str(), json.as_str())
                    .context("Failed to insert frontmatter")?;
            }
        }
        {
            let mut tag_table = write_txn.open_table(TAG_INDEX)
                .context("Failed to open tag_index table")?;
            for (tag, files) in &tag_to_files {
                let json = serde_json::to_string(files).expect(crate::errors::JSON_SERIALIZE);
                tag_table.insert(tag.as_str(), json.as_str())
                    .context("Failed to insert tag index")?;
            }
        }
        write_txn.commit().context("Failed to commit tags database")?;

        Ok(())
    }

    fn clean(&self, product: &Product, verbose: bool) -> Result<usize> {
        clean_outputs(product, crate::processors::names::TAGS, verbose)
    }

    fn config_json(&self) -> Option<String> {
        serde_json::to_string(&self.config).ok()
    }
}

/// Parse YAML frontmatter from a markdown file.
/// Looks for content between `---` markers at the start of the file.
fn parse_frontmatter(content: &str) -> Option<serde_json::Value> {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return None;
    }

    // Find the closing ---
    let after_first = &trimmed[3..];
    let rest = after_first.trim_start_matches(|c: char| c == '\r' || c == '\n');
    let end_pos = rest.find("\n---")?;
    let yaml_block = &rest[..end_pos];

    Some(parse_simple_yaml(yaml_block))
}

/// Parse simple YAML key-value pairs and lists.
/// Supports:
///   key: value
///   key:
///     - item1
///     - item2
fn parse_simple_yaml(block: &str) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    let mut current_key: Option<String> = None;
    let mut current_list: Vec<serde_json::Value> = Vec::new();
    let mut in_list = false;

    for line in block.lines() {
        let stripped = line.trim_end();

        // Check for list item (indented with -)
        if let Some(item) = stripped.strip_prefix(|c: char| c == ' ' || c == '\t') {
            if let Some(item) = item.trim_start().strip_prefix("- ") {
                if in_list {
                    current_list.push(serde_json::Value::String(item.trim().to_string()));
                    continue;
                }
            }
        }

        // If we were building a list, save it
        if in_list {
            if let Some(key) = current_key.take() {
                map.insert(key, serde_json::Value::Array(std::mem::take(&mut current_list)));
            }
            in_list = false;
        }

        // Parse key: value
        if let Some((key, value)) = stripped.split_once(':') {
            let key = key.trim().to_string();
            let value = value.trim();

            if value.is_empty() {
                // Start of a list
                current_key = Some(key);
                in_list = true;
                current_list.clear();
            } else {
                map.insert(key, serde_json::Value::String(value.to_string()));
            }
        }
    }

    // Flush any trailing list
    if in_list {
        if let Some(key) = current_key.take() {
            map.insert(key, serde_json::Value::Array(current_list));
        }
    }

    serde_json::Value::Object(map)
}

/// Open the tags database for reading. Used by the `rsb tags` CLI subcommand.
pub fn open_tags_db(db_path: &str) -> Result<redb::Database> {
    let path = std::path::Path::new(db_path);
    if !path.exists() {
        anyhow::bail!("Tags database not found: {}. Run 'rsb build' first.", db_path);
    }
    redb::Database::open(path)
        .with_context(|| format!("Failed to open tags database: {}", db_path))
}

/// List all unique tags from the database.
pub fn list_tags(db_path: &str) -> Result<()> {
    let db = open_tags_db(db_path)?;
    let read_txn = db.begin_read().context("Failed to begin read transaction")?;
    let table = read_txn.open_table(TAG_INDEX).context("Failed to open tag_index table")?;

    let mut tags: Vec<String> = Vec::new();
    let iter = table.iter().context("Failed to iterate tag_index")?;
    for entry in iter {
        let (key, _) = entry.context("Failed to read tag entry")?;
        tags.push(key.value().to_string());
    }
    tags.sort();

    for tag in &tags {
        println!("{}", tag);
    }

    Ok(())
}

/// List tags containing the given substring.
pub fn grep_tags(db_path: &str, text: &str) -> Result<()> {
    let db = open_tags_db(db_path)?;
    let read_txn = db.begin_read().context("Failed to begin read transaction")?;
    let table = read_txn.open_table(TAG_INDEX).context("Failed to open tag_index table")?;

    let mut matches: Vec<String> = Vec::new();
    let iter = table.iter().context("Failed to iterate tag_index")?;
    for entry in iter {
        let (key, _) = entry.context("Failed to read tag entry")?;
        let tag = key.value();
        if tag.contains(text) {
            matches.push(tag.to_string());
        }
    }
    matches.sort();

    for tag in &matches {
        println!("{}", tag);
    }

    Ok(())
}

/// List files matching given tags. AND by default, OR if `use_or` is true.
pub fn files_for_tags(db_path: &str, tags: &[String], use_or: bool) -> Result<()> {
    let db = open_tags_db(db_path)?;
    let read_txn = db.begin_read().context("Failed to begin read transaction")?;
    let table = read_txn.open_table(TAG_INDEX).context("Failed to open tag_index table")?;

    let mut result: Option<std::collections::BTreeSet<String>> = None;

    for tag in tags {
        let files: std::collections::BTreeSet<String> = match table.get(tag.as_str()).context("Failed to query tag")? {
            Some(value) => {
                let v: Vec<String> = serde_json::from_str(value.value())
                    .context("Failed to parse tag file list")?;
                v.into_iter().collect()
            }
            None => std::collections::BTreeSet::new(),
        };
        result = Some(match result {
            Some(acc) => {
                if use_or {
                    acc.union(&files).cloned().collect()
                } else {
                    acc.intersection(&files).cloned().collect()
                }
            }
            None => files,
        });
    }

    let files = result.unwrap_or_default();
    if files.is_empty() {
        let mode = if use_or { "any" } else { "all" };
        println!("No files found matching {} tags: {}", mode, tags.join(", "));
    } else {
        for file in &files {
            println!("{}", file);
        }
    }

    Ok(())
}

/// Show each tag with its file count, sorted by frequency (descending).
pub fn count_tags(db_path: &str) -> Result<()> {
    let tag_counts = load_tag_counts(db_path)?;
    let mut entries: Vec<(String, usize)> = tag_counts.into_iter().collect();
    entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    for (tag, count) in &entries {
        println!("{:>4}  {}", count, tag);
    }

    Ok(())
}

/// Show tags grouped by prefix/category.
pub fn tree_tags(db_path: &str) -> Result<()> {
    let tag_counts = load_tag_counts(db_path)?;

    // Split into key=value groups and bare tags
    let mut groups: std::collections::BTreeMap<String, Vec<(String, usize)>> = std::collections::BTreeMap::new();
    let mut bare: Vec<(String, usize)> = Vec::new();

    for (tag, count) in &tag_counts {
        if let Some((key, value)) = tag.split_once('=') {
            groups.entry(key.to_string())
                .or_default()
                .push((value.to_string(), *count));
        } else {
            bare.push((tag.clone(), *count));
        }
    }

    // Print key=value groups
    for (key, mut values) in groups {
        values.sort_by(|a, b| a.0.cmp(&b.0));
        println!("{}=", key);
        for (value, count) in &values {
            println!("  {:>4}  {}", count, value);
        }
    }

    // Print bare tags
    if !bare.is_empty() {
        bare.sort_by(|a, b| a.0.cmp(&b.0));
        println!("(bare tags)");
        for (tag, count) in &bare {
            println!("  {:>4}  {}", count, tag);
        }
    }

    Ok(())
}

/// Show statistics about the tags database.
pub fn stats_tags(db_path: &str) -> Result<()> {
    let db = open_tags_db(db_path)?;
    let read_txn = db.begin_read().context("Failed to begin read transaction")?;

    // Count indexed files
    let fm_table = read_txn.open_table(FRONTMATTER).context("Failed to open frontmatter table")?;
    let file_count = fm_table.len().context("Failed to count frontmatter entries")?;

    // Count and classify tags, and sum total associations
    let tag_table = read_txn.open_table(TAG_INDEX).context("Failed to open tag_index table")?;
    let mut bare_count: u64 = 0;
    let mut kv_count: u64 = 0;
    let mut total_associations: u64 = 0;
    let iter = tag_table.iter().context("Failed to iterate tag_index")?;
    for entry in iter {
        let (key, value) = entry.context("Failed to read tag entry")?;
        if key.value().contains('=') {
            kv_count += 1;
        } else {
            bare_count += 1;
        }
        let files: Vec<String> = serde_json::from_str(value.value())
            .context("Failed to parse tag file list")?;
        total_associations += files.len() as u64;
    }

    let unique_tags = bare_count + kv_count;
    println!("Files indexed:    {}", file_count);
    println!("Tag assignments:  {}", total_associations);
    println!("Unique tags:      {}", unique_tags);
    println!("  bare tags:      {}", bare_count);
    println!("  key=value:      {}", kv_count);

    Ok(())
}

/// List all tags for a specific file.
pub fn tags_for_file(db_path: &str, path: &str) -> Result<()> {
    let db = open_tags_db(db_path)?;
    let read_txn = db.begin_read().context("Failed to begin read transaction")?;
    let table = read_txn.open_table(TAG_INDEX).context("Failed to open tag_index table")?;

    let mut file_tags: Vec<String> = Vec::new();
    let iter = table.iter().context("Failed to iterate tag_index")?;
    for entry in iter {
        let (key, value) = entry.context("Failed to read tag entry")?;
        let files: Vec<String> = serde_json::from_str(value.value())
            .context("Failed to parse tag file list")?;
        if files.iter().any(|f| f == path || f.ends_with(path)) {
            file_tags.push(key.value().to_string());
        }
    }
    file_tags.sort();

    if file_tags.is_empty() {
        println!("No tags found for: {}", path);
    } else {
        for tag in &file_tags {
            println!("{}", tag);
        }
    }

    Ok(())
}

/// List tags in .tags that are not used by any file.
pub fn unused_tags(db_path: &str, tags_file: &str) -> Result<()> {
    let tags_file_path = Path::new(tags_file);
    if !tags_file_path.exists() {
        bail!("Tags file not found: {}", tags_file);
    }

    let allowed = load_tags_file(tags_file_path)?;
    let db_tags = load_all_tags(db_path)?;

    let mut unused: Vec<&String> = allowed.iter()
        .filter(|t| !db_tags.contains(*t))
        .collect();
    unused.sort();

    if unused.is_empty() {
        println!("All tags in {} are in use.", tags_file);
    } else {
        for tag in &unused {
            println!("{}", tag);
        }
    }

    Ok(())
}

/// Validate tags against .tags file without building.
pub fn validate_tags(db_path: &str, tags_file: &str) -> Result<()> {
    let tags_file_path = Path::new(tags_file);
    if !tags_file_path.exists() {
        bail!("Tags file not found: {}. Run 'rsb tags init' to create one.", tags_file);
    }

    let allowed = load_tags_file(tags_file_path)?;
    let db_tags = load_all_tags(db_path)?;
    let tag_counts = load_tag_counts(db_path)?;

    let mut unknown: Vec<String> = db_tags.iter()
        .filter(|t| !tag_matches_allowed(t, &allowed))
        .cloned()
        .collect();
    unknown.sort();

    if unknown.is_empty() {
        println!("All tags are valid.");
    } else {
        let mut msg = format!("{} unknown tag(s) found:\n", unknown.len());
        for tag in &unknown {
            let count = tag_counts.get(tag).copied().unwrap_or(0);
            msg.push_str(&format!("  {} ({} file(s))", tag, count));
            // Suggest similar tags
            if let Some(suggestion) = find_similar_tag(tag, &allowed) {
                msg.push_str(&format!(" - did you mean '{}'?", suggestion));
            }
            msg.push('\n');
        }
        bail!("{}", msg.trim_end());
    }

    Ok(())
}

/// Generate .tags file from current tag union.
pub fn init_tags(db_path: &str, tags_file: &str) -> Result<()> {
    let tags_file_path = Path::new(tags_file);
    if tags_file_path.exists() {
        bail!("{} already exists. Use 'rsb tags sync' to update it.", tags_file);
    }

    let tags = load_all_tags_sorted(db_path)?;
    let content = tags.join("\n") + "\n";
    fs::write(tags_file_path, content)
        .with_context(|| format!("Failed to write {}", tags_file))?;
    println!("Created {} with {} tags.", tags_file, tags.len());

    Ok(())
}

/// Add a tag to the .tags file (sorted, deduplicated).
pub fn add_tag(tags_file: &str, tag: &str) -> Result<()> {
    let tags_file_path = Path::new(tags_file);
    let mut tags = if tags_file_path.exists() {
        load_tags_file_sorted(tags_file_path)?
    } else {
        Vec::new()
    };

    if tags.iter().any(|t| t == tag) {
        println!("Tag '{}' already in {}.", tag, tags_file);
        return Ok(());
    }

    tags.push(tag.to_string());
    tags.sort();
    write_tags_file(tags_file_path, &tags)?;
    println!("Added '{}' to {}.", tag, tags_file);

    Ok(())
}

/// Remove a tag from the .tags file.
pub fn remove_tag(tags_file: &str, tag: &str) -> Result<()> {
    let tags_file_path = Path::new(tags_file);
    if !tags_file_path.exists() {
        bail!("Tags file not found: {}", tags_file);
    }

    let mut tags = load_tags_file_sorted(tags_file_path)?;
    let before_len = tags.len();
    tags.retain(|t| t != tag);

    if tags.len() == before_len {
        println!("Tag '{}' not found in {}.", tag, tags_file);
    } else {
        write_tags_file(tags_file_path, &tags)?;
        println!("Removed '{}' from {}.", tag, tags_file);
    }

    Ok(())
}

/// Sync .tags file with current tag union.
pub fn sync_tags(db_path: &str, tags_file: &str, prune: bool) -> Result<()> {
    let tags_file_path = Path::new(tags_file);
    let db_tags: HashSet<String> = load_all_tags(db_path)?;

    let existing = if tags_file_path.exists() {
        load_tags_file(tags_file_path)?
    } else {
        HashSet::new()
    };

    let added_count = db_tags.iter().filter(|t| !existing.contains(*t)).count();
    let removed_count = if prune {
        existing.iter().filter(|t| !db_tags.contains(*t)).count()
    } else {
        0
    };

    // Build final set
    let mut final_tags: HashSet<String> = existing;
    // Add all db tags
    for tag in &db_tags {
        final_tags.insert(tag.clone());
    }
    // Prune tags not in db
    if prune {
        final_tags.retain(|t| db_tags.contains(t));
    }

    let mut sorted: Vec<String> = final_tags.into_iter().collect();
    sorted.sort();
    write_tags_file(tags_file_path, &sorted)?;

    println!("Synced {} ({} tags total, {} added{})",
        tags_file,
        sorted.len(),
        added_count,
        if prune { format!(", {} pruned", removed_count) } else { String::new() },
    );

    Ok(())
}

// --- Helper functions ---

/// Load all tags from the database as a HashSet.
fn load_all_tags(db_path: &str) -> Result<HashSet<String>> {
    let db = open_tags_db(db_path)?;
    let read_txn = db.begin_read().context("Failed to begin read transaction")?;
    let table = read_txn.open_table(TAG_INDEX).context("Failed to open tag_index table")?;

    let mut tags = HashSet::new();
    let iter = table.iter().context("Failed to iterate tag_index")?;
    for entry in iter {
        let (key, _) = entry.context("Failed to read tag entry")?;
        tags.insert(key.value().to_string());
    }

    Ok(tags)
}

/// Load all tags from the database as a sorted Vec.
fn load_all_tags_sorted(db_path: &str) -> Result<Vec<String>> {
    let mut tags: Vec<String> = load_all_tags(db_path)?.into_iter().collect();
    tags.sort();
    Ok(tags)
}

/// Load tag -> file count mapping from the database.
fn load_tag_counts(db_path: &str) -> Result<HashMap<String, usize>> {
    let db = open_tags_db(db_path)?;
    let read_txn = db.begin_read().context("Failed to begin read transaction")?;
    let table = read_txn.open_table(TAG_INDEX).context("Failed to open tag_index table")?;

    let mut counts = HashMap::new();
    let iter = table.iter().context("Failed to iterate tag_index")?;
    for entry in iter {
        let (key, value) = entry.context("Failed to read tag entry")?;
        let files: Vec<String> = serde_json::from_str(value.value())
            .context("Failed to parse tag file list")?;
        counts.insert(key.value().to_string(), files.len());
    }

    Ok(counts)
}

/// Parse a .tags file into a HashSet, skipping comments and blank lines.
fn load_tags_file(path: &Path) -> Result<HashSet<String>> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    Ok(content
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| line.to_string())
        .collect())
}

/// Parse a .tags file into a sorted Vec, skipping comments and blank lines.
fn load_tags_file_sorted(path: &Path) -> Result<Vec<String>> {
    let mut tags: Vec<String> = load_tags_file(path)?.into_iter().collect();
    tags.sort();
    Ok(tags)
}

/// Write a sorted list of tags to a .tags file, one per line.
fn write_tags_file(path: &Path, tags: &[String]) -> Result<()> {
    let content = tags.join("\n") + "\n";
    fs::write(path, content)
        .with_context(|| format!("Failed to write {}", path.display()))
}

/// Check if a tag matches the allowed set, supporting wildcard patterns.
/// Patterns like `duration_days=*` match any tag starting with `duration_days=`.
fn tag_matches_allowed(tag: &str, allowed: &HashSet<String>) -> bool {
    if allowed.contains(tag) {
        return true;
    }
    // Check wildcard patterns
    for pattern in allowed {
        if let Some(prefix) = pattern.strip_suffix('*') {
            if tag.starts_with(prefix) {
                return true;
            }
        }
    }
    false
}

/// Find the most similar tag in the allowed set using Levenshtein distance.
/// Returns None if no tag is within distance 3.
fn find_similar_tag(tag: &str, allowed: &HashSet<String>) -> Option<String> {
    let mut best: Option<(String, usize)> = None;
    for candidate in allowed {
        // Skip wildcard patterns
        if candidate.ends_with('*') {
            continue;
        }
        let dist = levenshtein(tag, candidate);
        if dist > 0 && dist <= 3 {
            if best.is_none() || dist < best.as_ref().unwrap().1 {
                best = Some((candidate.clone(), dist));
            }
        }
    }
    best.map(|(s, _)| s)
}

/// Compute Levenshtein edit distance between two strings.
fn levenshtein(a: &str, b: &str) -> usize {
    let a_len = a.len();
    let b_len = b.len();

    if a_len == 0 { return b_len; }
    if b_len == 0 { return a_len; }

    let mut prev: Vec<usize> = (0..=b_len).collect();
    let mut curr = vec![0; b_len + 1];

    for (i, ca) in a.chars().enumerate() {
        curr[0] = i + 1;
        for (j, cb) in b.chars().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            curr[j + 1] = (prev[j] + cost)
                .min(curr[j] + 1)
                .min(prev[j + 1] + 1);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[b_len]
}
