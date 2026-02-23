use anyhow::{Context, Result};
use redb::{ReadableDatabase, ReadableTable, ReadableTableMetadata, TableDefinition};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

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
        let mut inputs = Vec::with_capacity(files.len() + extra.len());
        inputs.extend(files);
        inputs.extend_from_slice(&extra);

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

/// List files that have all the given tags (AND semantics).
pub fn files_for_tags(db_path: &str, tags: &[String]) -> Result<()> {
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
            Some(acc) => acc.intersection(&files).cloned().collect(),
            None => files,
        });
    }

    let files = result.unwrap_or_default();
    if files.is_empty() {
        println!("No files found matching all tags: {}", tags.join(", "));
    } else {
        for file in &files {
            println!("{}", file);
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
