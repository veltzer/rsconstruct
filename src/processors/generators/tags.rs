use anyhow::{Context, Result, bail};
use redb::{ReadableDatabase, ReadableTable, ReadableTableMetadata, TableDefinition};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
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
        let mut tag_to_files: HashMap<String, BTreeSet<String>> = HashMap::new();

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
                                            .insert(file_key.clone());
                                    }
                                }
                            }
                            serde_json::Value::String(s) => {
                                let tag = format!("{}={}", key, s);
                                tag_to_files.entry(tag)
                                    .or_default()
                                    .insert(file_key.clone());
                            }
                            // parse_simple_yaml produces only String values for scalars,
                            // so Number/Bool/Null/Object cannot occur here.
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
                    unknown.push((tag.clone(), files.iter().cloned().collect()));
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

        // Delete old database to avoid stale entries from previous builds
        if output_path.exists() {
            fs::remove_file(output_path)
                .with_context(|| format!("Failed to remove old tags database: {}", output_path.display()))?;
        }
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
                let files_vec: Vec<&String> = files.iter().collect();
                let json = serde_json::to_string(&files_vec).expect(crate::errors::JSON_SERIALIZE);
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

/// Strip surrounding quotes (single or double) from a YAML value.
fn strip_yaml_quotes(s: &str) -> &str {
    if s.len() >= 2 {
        if (s.starts_with('"') && s.ends_with('"'))
            || (s.starts_with('\'') && s.ends_with('\''))
        {
            return &s[1..s.len() - 1];
        }
    }
    s
}

/// Parse simple YAML key-value pairs and lists.
/// Supports:
///   key: value           (scalar, including values with colons like URLs)
///   key: "quoted value"
///   key: [a, b, c]       (inline list)
///   key:
///     - item1
///     - item2
///
/// Limitations: no multi-line strings (| or >), no nested objects, no anchors.
fn parse_simple_yaml(block: &str) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    let mut current_key: Option<String> = None;
    let mut current_list: Vec<serde_json::Value> = Vec::new();
    let mut in_list = false;

    for line in block.lines() {
        let stripped = line.trim_end();

        // Check for list item: optional leading whitespace followed by "- "
        let trimmed = stripped.trim_start();
        if let Some(item) = trimmed.strip_prefix("- ") {
            if in_list {
                let val = strip_yaml_quotes(item.trim());
                current_list.push(serde_json::Value::String(val.to_string()));
                continue;
            }
        }

        // If we were building a list, save it
        if in_list {
            if let Some(key) = current_key.take() {
                map.insert(key, serde_json::Value::Array(std::mem::take(&mut current_list)));
            }
            in_list = false;
        }

        // Parse key: value — split on first ": " (colon+space) to preserve colons in values
        if let Some((key, value)) = stripped.split_once(": ") {
            let key = key.trim().to_string();
            let value = value.trim();

            if value.is_empty() {
                // Start of a multi-line list
                current_key = Some(key);
                in_list = true;
                current_list.clear();
            } else if value.starts_with('[') && value.ends_with(']') {
                // Inline list: [item1, item2, item3]
                let inner = &value[1..value.len() - 1];
                if inner.trim().is_empty() {
                    map.insert(key, serde_json::Value::Array(Vec::new()));
                } else {
                    let items: Vec<serde_json::Value> = inner.split(',')
                        .map(|s| {
                            let val = strip_yaml_quotes(s.trim());
                            serde_json::Value::String(val.to_string())
                        })
                        .collect();
                    map.insert(key, serde_json::Value::Array(items));
                }
            } else {
                let val = strip_yaml_quotes(value);
                map.insert(key, serde_json::Value::String(val.to_string()));
            }
        } else if let Some((key, _)) = stripped.split_once(':') {
            // Key with no value after colon (no space) — start of a multi-line list
            let key = key.trim().to_string();
            current_key = Some(key);
            in_list = true;
            current_list.clear();
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
    let tags = load_all_tags_sorted(db_path)?;
    if crate::json_output::is_json_mode() {
        println!("{}", serde_json::to_string(&tags).expect(crate::errors::JSON_SERIALIZE));
    } else {
        for tag in &tags {
            println!("{}", tag);
        }
    }
    Ok(())
}

/// Search for tags containing a substring.
pub fn grep_tags(db_path: &str, text: &str, ignore_case: bool) -> Result<()> {
    let all_tags = load_all_tags_sorted(db_path)?;
    let needle = if ignore_case { text.to_lowercase() } else { text.to_string() };
    let matches: Vec<&String> = all_tags.iter()
        .filter(|t| {
            if ignore_case {
                t.to_lowercase().contains(&needle)
            } else {
                t.contains(text)
            }
        })
        .collect();
    if crate::json_output::is_json_mode() {
        println!("{}", serde_json::to_string(&matches).expect(crate::errors::JSON_SERIALIZE));
    } else {
        for tag in &matches {
            println!("{}", tag);
        }
    }
    Ok(())
}

/// List files matching given tags. AND by default, OR if `use_or` is true.
pub fn files_for_tags(db_path: &str, tags: &[String], use_or: bool) -> Result<()> {
    let db = open_tags_db(db_path)?;
    let read_txn = db.begin_read().context("Failed to begin read transaction")?;
    let table = read_txn.open_table(TAG_INDEX).context("Failed to open tag_index table")?;

    let mut result: Option<BTreeSet<String>> = None;

    for tag in tags {
        let files: BTreeSet<String> = match table.get(tag.as_str()).context("Failed to query tag")? {
            Some(value) => {
                let v: Vec<String> = serde_json::from_str(value.value())
                    .context("Failed to parse tag file list")?;
                v.into_iter().collect()
            }
            None => BTreeSet::new(),
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

    let files: Vec<String> = result.unwrap_or_default().into_iter().collect();
    if crate::json_output::is_json_mode() {
        println!("{}", serde_json::to_string(&files).expect(crate::errors::JSON_SERIALIZE));
    } else if files.is_empty() {
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

    if crate::json_output::is_json_mode() {
        let json_entries: Vec<serde_json::Value> = entries.iter()
            .map(|(tag, count)| serde_json::json!({"tag": tag, "count": count}))
            .collect();
        println!("{}", serde_json::to_string(&json_entries).expect(crate::errors::JSON_SERIALIZE));
    } else {
        for (tag, count) in &entries {
            println!("{:>4}  {}", count, tag);
        }
    }

    Ok(())
}

/// Show tags grouped by prefix/category.
pub fn tree_tags(db_path: &str) -> Result<()> {
    let tag_counts = load_tag_counts(db_path)?;

    // Split into key=value groups and bare tags
    let mut groups: BTreeMap<String, Vec<(String, usize)>> = BTreeMap::new();
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

    if crate::json_output::is_json_mode() {
        let mut json_groups: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();
        for (key, mut values) in groups {
            values.sort_by(|a, b| a.0.cmp(&b.0));
            let entries: Vec<serde_json::Value> = values.iter()
                .map(|(v, c)| serde_json::json!({"value": v, "count": c}))
                .collect();
            json_groups.insert(key, serde_json::Value::Array(entries));
        }
        bare.sort_by(|a, b| a.0.cmp(&b.0));
        let bare_entries: Vec<serde_json::Value> = bare.iter()
            .map(|(t, c)| serde_json::json!({"tag": t, "count": c}))
            .collect();
        json_groups.insert("_bare".to_string(), serde_json::Value::Array(bare_entries));
        println!("{}", serde_json::to_string(&json_groups).expect(crate::errors::JSON_SERIALIZE));
    } else {
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
    }

    Ok(())
}

/// Show statistics about the tags database.
pub fn stats_tags(db_path: &str) -> Result<()> {
    let db = open_tags_db(db_path)?;
    let read_txn = db.begin_read().context("Failed to begin read transaction")?;

    // Count indexed files from the frontmatter table
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

    if crate::json_output::is_json_mode() {
        let stats = serde_json::json!({
            "files_indexed": file_count,
            "tag_assignments": total_associations,
            "unique_tags": unique_tags,
            "bare_tags": bare_count,
            "kv_tags": kv_count,
        });
        println!("{}", serde_json::to_string(&stats).expect(crate::errors::JSON_SERIALIZE));
    } else {
        println!("Files indexed:    {}", file_count);
        println!("Tag assignments:  {}", total_associations);
        println!("Unique tags:      {}", unique_tags);
        println!("  bare tags:      {}", bare_count);
        println!("  key=value:      {}", kv_count);
    }

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
        if files.iter().any(|f| path_matches(f, path)) {
            file_tags.push(key.value().to_string());
        }
    }
    file_tags.sort();

    if crate::json_output::is_json_mode() {
        println!("{}", serde_json::to_string(&file_tags).expect(crate::errors::JSON_SERIALIZE));
    } else if file_tags.is_empty() {
        eprintln!("No tags found for: {}", path);
    } else {
        for tag in &file_tags {
            println!("{}", tag);
        }
    }

    Ok(())
}

/// Show the raw frontmatter for a specific file.
pub fn frontmatter_for_file(db_path: &str, path: &str) -> Result<()> {
    let db = open_tags_db(db_path)?;
    let read_txn = db.begin_read().context("Failed to begin read transaction")?;
    let table = read_txn.open_table(FRONTMATTER).context("Failed to open frontmatter table")?;

    // Try exact match first, then suffix match
    let mut found_key: Option<String> = None;
    let mut found_value: Option<String> = None;

    if let Some(value) = table.get(path).context("Failed to query frontmatter")? {
        found_key = Some(path.to_string());
        found_value = Some(value.value().to_string());
    } else {
        // Suffix match with path boundary — collect all matches to detect ambiguity
        let mut all_matches: Vec<(String, String)> = Vec::new();
        let iter = table.iter().context("Failed to iterate frontmatter")?;
        for entry in iter {
            let (key, value) = entry.context("Failed to read frontmatter entry")?;
            if path_matches(key.value(), path) {
                all_matches.push((key.value().to_string(), value.value().to_string()));
            }
        }
        if all_matches.len() > 1 {
            eprintln!("Warning: '{}' matches {} files, showing first:", path, all_matches.len());
            for (k, _) in &all_matches {
                eprintln!("  {}", k);
            }
        }
        if let Some((k, v)) = all_matches.into_iter().next() {
            found_key = Some(k);
            found_value = Some(v);
        }
    }

    match (found_key, found_value) {
        (Some(key), Some(json)) => {
            if crate::json_output::is_json_mode() {
                let fm_value: serde_json::Value = serde_json::from_str(&json)
                    .context("Failed to parse stored frontmatter")?;
                let output = serde_json::json!({"file": key, "frontmatter": fm_value});
                println!("{}", serde_json::to_string(&output).expect(crate::errors::JSON_SERIALIZE));
            } else {
                println!("{}:", key);
                let value: serde_json::Value = serde_json::from_str(&json)
                    .context("Failed to parse stored frontmatter")?;
                println!("{}", serde_json::to_string_pretty(&value).expect(crate::errors::JSON_SERIALIZE));
            }
        }
        _ => {
            if crate::json_output::is_json_mode() {
                println!("{}", serde_json::json!({"file": null, "frontmatter": null}));
            } else {
                println!("No frontmatter found for: {}", path);
            }
        }
    }

    Ok(())
}

/// List tags in .tags that are not used by any file.
/// If `strict` is true, returns an error when unused tags are found (for CI).
pub fn unused_tags(db_path: &str, tags_file: &str, strict: bool) -> Result<()> {
    let tags_file_path = Path::new(tags_file);
    if !tags_file_path.exists() {
        bail!("Tags file not found: {}", tags_file);
    }

    let allowed = load_tags_file(tags_file_path)?;
    let db_tags = load_all_tags(db_path)?;

    let mut unused: Vec<&String> = allowed.iter()
        .filter(|t| !pattern_matches_any_tag(t, &db_tags))
        .collect();
    unused.sort();

    if strict && !unused.is_empty() {
        // In strict mode, report via error (nonzero exit) with full details
        let mut msg = format!("{} unused tag(s) in {}:\n", unused.len(), tags_file);
        for tag in &unused {
            msg.push_str(&format!("  {}\n", tag));
        }
        bail!("{}", msg.trim_end());
    }

    if crate::json_output::is_json_mode() {
        println!("{}", serde_json::to_string(&unused).expect(crate::errors::JSON_SERIALIZE));
    } else if unused.is_empty() {
        println!("All tags in {} are in use.", tags_file);
    } else {
        println!("{} unused tag(s) in {}:", unused.len(), tags_file);
        for tag in &unused {
            println!("  {}", tag);
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
    // Single db open: get both tags and counts in one pass
    let tag_counts = load_tag_counts(db_path)?;

    let mut unknown: Vec<String> = tag_counts.keys()
        .filter(|t| !tag_matches_allowed(t, &allowed))
        .cloned()
        .collect();
    unknown.sort();

    if unknown.is_empty() {
        if crate::json_output::is_json_mode() {
            println!("{}", serde_json::json!({"valid": true, "unknown_count": 0}));
        } else {
            println!("All tags are valid.");
        }
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
    write_tags_file(tags_file_path, &tags)?;
    if crate::json_output::is_json_mode() {
        println!("{}", serde_json::json!({"action": "init", "file": tags_file, "count": tags.len()}));
    } else {
        println!("Created {} with {} tags.", tags_file, tags.len());
    }

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
        if crate::json_output::is_json_mode() {
            println!("{}", serde_json::json!({"action": "add", "tag": tag, "added": false}));
        } else {
            println!("Tag '{}' already in {}.", tag, tags_file);
        }
        return Ok(());
    }

    tags.push(tag.to_string());
    tags.sort();
    write_tags_file(tags_file_path, &tags)?;
    if crate::json_output::is_json_mode() {
        println!("{}", serde_json::json!({"action": "add", "tag": tag, "added": true}));
    } else {
        println!("Added '{}' to {}.", tag, tags_file);
    }

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

    let removed = tags.len() < before_len;
    if removed {
        write_tags_file(tags_file_path, &tags)?;
    }
    if crate::json_output::is_json_mode() {
        println!("{}", serde_json::json!({"action": "remove", "tag": tag, "removed": removed}));
    } else if removed {
        println!("Removed '{}' from {}.", tag, tags_file);
    } else {
        println!("Tag '{}' not found in {}.", tag, tags_file);
    }

    Ok(())
}

/// Sync .tags file with current tag union.
pub fn sync_tags(db_path: &str, tags_file: &str, prune: bool, verbose: bool) -> Result<()> {
    let tags_file_path = Path::new(tags_file);
    let db_tags: HashSet<String> = load_all_tags(db_path)?;

    let existing = if tags_file_path.exists() {
        load_tags_file(tags_file_path)?
    } else {
        HashSet::new()
    };

    let mut added: Vec<String> = db_tags.iter()
        .filter(|t| !tag_matches_allowed(t, &existing))
        .cloned()
        .collect();
    added.sort();

    let mut removed: Vec<String> = if prune {
        existing.iter()
            .filter(|t| !pattern_matches_any_tag(t, &db_tags))
            .cloned()
            .collect()
    } else {
        Vec::new()
    };
    removed.sort();

    // Build final set
    let mut final_tags: HashSet<String> = existing;
    for tag in &added {
        final_tags.insert(tag.clone());
    }
    for tag in &removed {
        final_tags.remove(tag);
    }

    let mut sorted: Vec<String> = final_tags.into_iter().collect();
    sorted.sort();
    if !added.is_empty() || !removed.is_empty() {
        write_tags_file(tags_file_path, &sorted)?;
    }

    if crate::json_output::is_json_mode() {
        println!("{}", serde_json::json!({
            "action": "sync",
            "file": tags_file,
            "total": sorted.len(),
            "added": added,
            "removed": removed,
        }));
    } else {
        println!("Synced {} ({} tags total, {} added{})",
            tags_file,
            sorted.len(),
            added.len(),
            if prune { format!(", {} pruned", removed.len()) } else { String::new() },
        );

        if verbose {
            for tag in &added {
                println!("  + {}", tag);
            }
            for tag in &removed {
                println!("  - {}", tag);
            }
        }
    }

    Ok(())
}

// --- Helper functions ---

/// Check if a stored file path matches a user-provided path.
/// Matches exactly, or as a suffix after a `/` path separator.
fn path_matches(stored: &str, query: &str) -> bool {
    if stored == query {
        return true;
    }
    // Suffix match: stored must end with /query.
    // Subtraction is safe: the guard ensures stored.len() > query.len(),
    // so stored.len() - query.len() - 1 cannot underflow.
    if stored.len() > query.len() {
        let boundary = stored.len() - query.len() - 1;
        if stored.as_bytes().get(boundary) == Some(&b'/')
            && stored.ends_with(query)
        {
            return true;
        }
    }
    false
}

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

/// Write a sorted list of tags to a .tags file, one per line with a header comment.
fn write_tags_file(path: &Path, tags: &[String]) -> Result<()> {
    let mut content = String::from("# Allowed tags for rsb frontmatter validation\n# One tag per line. Wildcards supported (e.g. duration_days=*)\n");
    for tag in tags {
        content.push_str(tag);
        content.push('\n');
    }
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

/// Check if a pattern from .tags matches any actual tag in the database.
/// For literal tags, checks direct membership.
/// For wildcard patterns like `duration_days=*`, checks if any db tag has that prefix.
fn pattern_matches_any_tag(pattern: &str, db_tags: &HashSet<String>) -> bool {
    if let Some(prefix) = pattern.strip_suffix('*') {
        db_tags.iter().any(|t| t.starts_with(prefix))
    } else {
        db_tags.contains(pattern)
    }
}

/// Find the most similar tag in the allowed set using Levenshtein distance.
/// The threshold scales with tag length to avoid spurious matches on short tags.
fn find_similar_tag(tag: &str, allowed: &HashSet<String>) -> Option<String> {
    // Scale threshold: at least 1, at most 3, roughly tag_char_count/3
    let max_dist = (tag.chars().count() / 3).clamp(1, 3);
    let mut best: Option<(String, usize)> = None;
    for candidate in allowed {
        // Skip wildcard patterns
        if candidate.ends_with('*') {
            continue;
        }
        let dist = levenshtein(tag, candidate);
        if dist > 0 && dist <= max_dist {
            if best.is_none() || dist < best.as_ref().unwrap().1 {
                best = Some((candidate.clone(), dist));
            }
        }
    }
    best.map(|(s, _)| s)
}

/// Compute Levenshtein edit distance between two strings.
/// Uses char-level comparison for correct handling of non-ASCII input.
fn levenshtein(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let a_len = a_chars.len();
    let b_len = b_chars.len();

    if a_len == 0 { return b_len; }
    if b_len == 0 { return a_len; }

    let mut prev: Vec<usize> = (0..=b_len).collect();
    let mut curr = vec![0; b_len + 1];

    for (i, ca) in a_chars.iter().enumerate() {
        curr[0] = i + 1;
        for (j, cb) in b_chars.iter().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            curr[j + 1] = (prev[j] + cost)
                .min(curr[j] + 1)
                .min(prev[j + 1] + 1);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[b_len]
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- parse_frontmatter ---

    #[test]
    fn frontmatter_basic() {
        let content = "---\ntitle: Hello\ntags:\n  - foo\n---\n# Body\n";
        let fm = parse_frontmatter(content).unwrap();
        assert_eq!(fm["title"], "Hello");
        assert_eq!(fm["tags"][0], "foo");
    }

    #[test]
    fn frontmatter_no_markers() {
        assert!(parse_frontmatter("# Just a heading\n").is_none());
    }

    #[test]
    fn frontmatter_only_opening() {
        assert!(parse_frontmatter("---\ntitle: Hello\n").is_none());
    }

    #[test]
    fn frontmatter_empty_block() {
        // Empty frontmatter (no content between --- markers) returns None
        // because there is no "\n---" after the opening marker's newline
        let content = "---\n---\n# Body\n";
        assert!(parse_frontmatter(content).is_none());
    }

    // --- parse_simple_yaml ---

    #[test]
    fn yaml_scalar_values() {
        let block = "title: Hello World\nlevel: beginner";
        let v = parse_simple_yaml(block);
        assert_eq!(v["title"], "Hello World");
        assert_eq!(v["level"], "beginner");
    }

    #[test]
    fn yaml_colon_in_value() {
        let block = "url: https://example.com/path\ntime: 10:30";
        let v = parse_simple_yaml(block);
        assert_eq!(v["url"], "https://example.com/path");
        assert_eq!(v["time"], "10:30");
    }

    #[test]
    fn yaml_multiline_list() {
        let block = "tags:\n  - alpha\n  - beta\n  - gamma";
        let v = parse_simple_yaml(block);
        let tags = v["tags"].as_array().unwrap();
        assert_eq!(tags.len(), 3);
        assert_eq!(tags[0], "alpha");
        assert_eq!(tags[1], "beta");
        assert_eq!(tags[2], "gamma");
    }

    #[test]
    fn yaml_inline_list() {
        let block = "tags: [alpha, beta, gamma]";
        let v = parse_simple_yaml(block);
        let tags = v["tags"].as_array().unwrap();
        assert_eq!(tags.len(), 3);
        assert_eq!(tags[0], "alpha");
        assert_eq!(tags[2], "gamma");
    }

    #[test]
    fn yaml_empty_inline_list() {
        let block = "tags: []";
        let v = parse_simple_yaml(block);
        let tags = v["tags"].as_array().unwrap();
        assert!(tags.is_empty());
    }

    #[test]
    fn yaml_quoted_values() {
        let block = "name: \"quoted value\"\nother: 'single quoted'";
        let v = parse_simple_yaml(block);
        assert_eq!(v["name"], "quoted value");
        assert_eq!(v["other"], "single quoted");
    }

    #[test]
    fn yaml_list_no_leading_whitespace() {
        let block = "tags:\n- alpha\n- beta";
        let v = parse_simple_yaml(block);
        let tags = v["tags"].as_array().unwrap();
        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0], "alpha");
    }

    #[test]
    fn yaml_empty_block() {
        let v = parse_simple_yaml("");
        assert!(v.as_object().unwrap().is_empty());
    }

    #[test]
    fn yaml_key_no_space_after_colon() {
        // "tags:" with no space starts a list
        let block = "tags:\n  - item";
        let v = parse_simple_yaml(block);
        let tags = v["tags"].as_array().unwrap();
        assert_eq!(tags.len(), 1);
    }

    // --- strip_yaml_quotes ---

    #[test]
    fn strip_double_quotes() {
        assert_eq!(strip_yaml_quotes("\"hello\""), "hello");
    }

    #[test]
    fn strip_single_quotes() {
        assert_eq!(strip_yaml_quotes("'hello'"), "hello");
    }

    #[test]
    fn strip_no_quotes() {
        assert_eq!(strip_yaml_quotes("hello"), "hello");
    }

    #[test]
    fn strip_mismatched_quotes() {
        assert_eq!(strip_yaml_quotes("\"hello'"), "\"hello'");
    }

    #[test]
    fn strip_single_char() {
        assert_eq!(strip_yaml_quotes("x"), "x");
    }

    #[test]
    fn strip_empty() {
        assert_eq!(strip_yaml_quotes(""), "");
    }

    // --- path_matches ---

    #[test]
    fn path_exact_match() {
        assert!(path_matches("foo.md", "foo.md"));
    }

    #[test]
    fn path_suffix_match() {
        assert!(path_matches("sub/foo.md", "foo.md"));
        assert!(path_matches("a/b/foo.md", "b/foo.md"));
    }

    #[test]
    fn path_no_false_suffix() {
        // "barfoo.md" should NOT match query "foo.md"
        assert!(!path_matches("barfoo.md", "foo.md"));
        assert!(!path_matches("sub/barfoo.md", "foo.md"));
    }

    #[test]
    fn path_query_longer_than_stored() {
        assert!(!path_matches("foo.md", "sub/foo.md"));
    }

    #[test]
    fn path_empty_strings() {
        assert!(path_matches("", ""));
        assert!(!path_matches("foo", ""));
        // empty query with non-empty stored: stored.len() > 0, boundary would need -1
        // but stored.len() > query.len() is true, boundary = stored.len() - 0 - 1
        // stored.as_bytes().get(boundary) checks for '/'
    }

    // --- levenshtein ---

    #[test]
    fn levenshtein_identical() {
        assert_eq!(levenshtein("docker", "docker"), 0);
    }

    #[test]
    fn levenshtein_empty() {
        assert_eq!(levenshtein("", ""), 0);
        assert_eq!(levenshtein("abc", ""), 3);
        assert_eq!(levenshtein("", "abc"), 3);
    }

    #[test]
    fn levenshtein_one_edit() {
        assert_eq!(levenshtein("docker", "dockker"), 1);
        assert_eq!(levenshtein("python", "pyhton"), 2); // transposition = 2 edits
    }

    #[test]
    fn levenshtein_single_char() {
        assert_eq!(levenshtein("a", "b"), 1);
        assert_eq!(levenshtein("a", "a"), 0);
    }

    // --- tag_matches_allowed ---

    #[test]
    fn tag_allowed_exact() {
        let allowed: HashSet<String> = ["docker", "python"].iter().map(|s| s.to_string()).collect();
        assert!(tag_matches_allowed("docker", &allowed));
        assert!(!tag_matches_allowed("rust", &allowed));
    }

    #[test]
    fn tag_allowed_wildcard() {
        let allowed: HashSet<String> = ["level=*", "docker"].iter().map(|s| s.to_string()).collect();
        assert!(tag_matches_allowed("level=beginner", &allowed));
        assert!(tag_matches_allowed("level=advanced", &allowed));
        assert!(!tag_matches_allowed("difficulty=3", &allowed));
    }

    // --- pattern_matches_any_tag ---

    #[test]
    fn pattern_literal_match() {
        let db: HashSet<String> = ["docker", "python"].iter().map(|s| s.to_string()).collect();
        assert!(pattern_matches_any_tag("docker", &db));
        assert!(!pattern_matches_any_tag("rust", &db));
    }

    #[test]
    fn pattern_wildcard_match() {
        let db: HashSet<String> = ["level=beginner", "level=advanced"].iter().map(|s| s.to_string()).collect();
        assert!(pattern_matches_any_tag("level=*", &db));
        assert!(!pattern_matches_any_tag("difficulty=*", &db));
    }
}
