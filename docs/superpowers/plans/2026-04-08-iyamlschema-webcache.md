# iyamlschema Processor + Web Cache Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a native `iyamlschema` processor that validates YAML files against JSON schemas (fetched from URLs embedded in the YAML), with property ordering checks, and a `webcache` CLI subcommand for managing cached HTTP responses.

**Architecture:** The processor reads each YAML file, extracts the `$schema` URL, fetches the schema (caching responses on disk under `.rsconstruct/webcache/`), validates the data against the schema using the `jsonschema` crate, and checks property ordering against `propertyOrdering` fields in the schema. A new `src/webcache.rs` module handles HTTP fetching and disk caching. A new `webcache` CLI subcommand provides `stats`, `clear`, and `list` operations.

**Tech Stack:** `ureq` (blocking HTTP client), `jsonschema` (already in Cargo.toml), `serde_yml` + `serde_json` (already in Cargo.toml), `sha2` + `hex` (already in Cargo.toml for cache key hashing)

---

## File Structure

| File | Action | Responsibility |
|------|--------|----------------|
| `Cargo.toml` | Modify | Add `ureq` dependency |
| `src/webcache.rs` | Create | HTTP fetch + disk cache (fetch, clear, stats, list) |
| `src/processors/checkers/iyamlschema.rs` | Create | Native YAML schema validator processor |
| `src/processors/checkers/mod.rs` | Modify | Add `mod iyamlschema` and `pub use` |
| `src/processors/mod.rs` | Modify | Re-export `IyamlschemaProcessor` |
| `src/config/processor_configs.rs` | Modify | Add `IyamlschemaConfig` |
| `src/registry.rs` | Modify | Add registry entry |
| `src/cli.rs` | Modify | Add `WebCache` command + `WebCacheAction` enum |
| `src/main.rs` | Modify | Add `mod webcache`, handle `WebCache` command |
| `tests/processors/iyamlschema.rs` | Create | Integration tests |
| `tests/processors/mod.rs` | Modify | Add `mod iyamlschema` |

---

### Task 1: Add `ureq` dependency

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Add ureq to Cargo.toml**

Add under `[dependencies]`:
```toml
ureq = "3"
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build`
Expected: compiles successfully

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "deps: add ureq HTTP client for web cache"
```

---

### Task 2: Create `src/webcache.rs` — disk-cached HTTP fetcher

**Files:**
- Create: `src/webcache.rs`
- Modify: `src/main.rs` (add `mod webcache`)

- [ ] **Step 1: Create the webcache module**

```rust
// src/webcache.rs
use anyhow::{Context, Result};
use sha2::{Sha256, Digest};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const WEBCACHE_DIR: &str = ".rsconstruct/webcache";

/// Return the cache directory path.
fn cache_dir() -> PathBuf {
    PathBuf::from(WEBCACHE_DIR)
}

/// Return the cache file path for a URL (hashed).
fn cache_path(url: &str) -> PathBuf {
    let mut hasher = Sha256::new();
    hasher.update(url.as_bytes());
    let hash = hex::encode(hasher.finalize());
    cache_dir().join(&hash[..2]).join(&hash[2..])
}

/// Fetch a URL, returning cached content if available.
/// Caches the response body on disk.
pub fn fetch(url: &str) -> Result<String> {
    let path = cache_path(url);
    if path.exists() {
        return fs::read_to_string(&path)
            .with_context(|| format!("Failed to read cached response for {}", url));
    }

    let body: String = ureq::get(url).call()
        .with_context(|| format!("Failed to fetch {}", url))?
        .body_mut().read_to_string()
        .with_context(|| format!("Failed to read response body from {}", url))?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create cache directory {}", parent.display()))?;
    }
    fs::write(&path, &body)
        .with_context(|| format!("Failed to write cache file {}", path.display()))?;

    Ok(body)
}

/// Clear the entire web cache. Returns the number of files removed.
pub fn clear() -> Result<usize> {
    let dir = cache_dir();
    if !dir.exists() {
        return Ok(0);
    }
    let mut count = 0;
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            let subdir = entry.path();
            for file in fs::read_dir(&subdir)? {
                let file = file?;
                if file.file_type()?.is_file() {
                    fs::remove_file(file.path())?;
                    count += 1;
                }
            }
            // Remove empty subdirectory
            let _ = fs::remove_dir(&subdir);
        }
    }
    let _ = fs::remove_dir(&dir);
    Ok(count)
}

/// Cache entry info for list/stats.
pub struct CacheEntry {
    pub url_hash: String,
    pub size: u64,
    pub modified: Option<SystemTime>,
}

/// List all cache entries.
pub fn list() -> Result<Vec<CacheEntry>> {
    let dir = cache_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut entries = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            let prefix = entry.file_name().to_string_lossy().to_string();
            for file in fs::read_dir(entry.path())? {
                let file = file?;
                if file.file_type()?.is_file() {
                    let rest = file.file_name().to_string_lossy().to_string();
                    let meta = file.metadata()?;
                    entries.push(CacheEntry {
                        url_hash: format!("{}{}", prefix, rest),
                        size: meta.len(),
                        modified: meta.modified().ok(),
                    });
                }
            }
        }
    }
    Ok(entries)
}

/// Return (total_bytes, entry_count).
pub fn stats() -> Result<(u64, usize)> {
    let entries = list()?;
    let total: u64 = entries.iter().map(|e| e.size).sum();
    Ok((total, entries.len()))
}
```

- [ ] **Step 2: Add `mod webcache` to main.rs**

In `src/main.rs`, add after the other `mod` declarations:

```rust
mod webcache;
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build`
Expected: compiles (with a warning about unused `webcache` which is fine for now)

- [ ] **Step 4: Commit**

```bash
git add src/webcache.rs src/main.rs
git commit -m "feat: add webcache module for disk-cached HTTP fetching"
```

---

### Task 3: Add `webcache` CLI subcommand

**Files:**
- Modify: `src/cli.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Add WebCacheAction enum to cli.rs**

Add after the `CacheAction` enum:

```rust
#[derive(Subcommand)]
pub enum WebCacheAction {
    /// Clear the web cache
    Clear,
    /// Show web cache statistics (size, entry count)
    Stats,
    /// List all cached entries
    List,
}
```

- [ ] **Step 2: Add WebCache variant to Commands enum**

Add in the `Commands` enum (alphabetically, after `Tools`):

```rust
    /// Manage the web request cache
    #[command(name = "webcache")]
    WebCache {
        #[command(subcommand)]
        action: WebCacheAction,
    },
```

- [ ] **Step 3: Handle WebCache command in main.rs**

Add the import of `WebCacheAction` in the `use cli::` line, then add the match arm in the `match cli.command` block:

```rust
        Commands::WebCache { action } => {
            match action {
                WebCacheAction::Clear => {
                    let count = webcache::clear()?;
                    println!("Removed {} cached entries.", count);
                }
                WebCacheAction::Stats => {
                    let (bytes, count) = webcache::stats()?;
                    println!("Web cache: {} ({} entries)",
                        humansize::format_size(bytes, humansize::BINARY), count);
                }
                WebCacheAction::List => {
                    let entries = webcache::list()?;
                    if entries.is_empty() {
                        println!("Web cache is empty.");
                    } else {
                        let mut builder = tabled::builder::Builder::new();
                        builder.push_record(["Hash", "Size"]);
                        for entry in &entries {
                            builder.push_record([
                                entry.url_hash.clone(),
                                humansize::format_size(entry.size, humansize::BINARY),
                            ]);
                        }
                        let table = builder.build()
                            .with(tabled::settings::Style::modern())
                            .to_string();
                        println!("{table}");
                    }
                }
            }
        }
```

- [ ] **Step 4: Verify it compiles and runs**

Run: `cargo build && cargo run -- webcache stats`
Expected: "Web cache: 0 B (0 entries)" or similar

- [ ] **Step 5: Commit**

```bash
git add src/cli.rs src/main.rs
git commit -m "feat: add webcache subcommand (clear, stats, list)"
```

---

### Task 4: Create `iyamlschema` processor config

**Files:**
- Modify: `src/config/processor_configs.rs`

- [ ] **Step 1: Add config using checker_config! macro**

Add after the existing `IyamllintConfig` definition:

```rust
checker_config!(IyamlschemaConfig, extensions: [".yml", ".yaml"]);
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build`

- [ ] **Step 3: Commit**

```bash
git add src/config/processor_configs.rs
git commit -m "feat: add IyamlschemaConfig"
```

---

### Task 5: Create `iyamlschema` processor

**Files:**
- Create: `src/processors/checkers/iyamlschema.rs`
- Modify: `src/processors/checkers/mod.rs`
- Modify: `src/processors/mod.rs`
- Modify: `src/registry.rs`

- [ ] **Step 1: Create the processor file**

```rust
// src/processors/checkers/iyamlschema.rs
use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::path::Path;

use crate::config::IyamlschemaConfig;
use crate::graph::Product;

pub struct IyamlschemaProcessor {
    config: IyamlschemaConfig,
}

impl IyamlschemaProcessor {
    pub fn new(config: IyamlschemaConfig) -> Self {
        Self { config }
    }

    fn execute_product(&self, product: &Product) -> Result<()> {
        self.check_files(&[product.primary_input()])
    }

    fn check_files(&self, files: &[&Path]) -> Result<()> {
        let mut errors = Vec::new();

        for file in files {
            if let Err(e) = self.validate_file(file) {
                errors.push(format!("{}: {}", file.display(), e));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            bail!("YAML schema validation failed:\n{}", errors.join("\n"))
        }
    }

    fn validate_file(&self, path: &Path) -> Result<()> {
        let contents = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;

        // Parse YAML into a JSON Value (for jsonschema validation)
        let data: Value = serde_yml::from_str(&contents)
            .with_context(|| format!("Failed to parse YAML in {}", path.display()))?;

        // Extract $schema URL
        let schema_url = data.get("$schema")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("no $schema field found"))?;

        // Fetch schema (cached)
        let schema_str = crate::webcache::fetch(schema_url)
            .with_context(|| format!("Failed to fetch schema {}", schema_url))?;
        let schema: Value = serde_json::from_str(&schema_str)
            .with_context(|| format!("Failed to parse schema from {}", schema_url))?;

        // Validate data against schema
        let validator = jsonschema::validator_for(&schema)
            .with_context(|| format!("Failed to compile schema from {}", schema_url))?;

        let validation_errors: Vec<String> = validator.iter_errors(&data)
            .map(|e| format!("  {}: {}", e.instance_path, e))
            .collect();

        if !validation_errors.is_empty() {
            bail!("schema validation errors:\n{}", validation_errors.join("\n"));
        }

        // Check property ordering
        let mut ordering_errors = Vec::new();
        check_property_ordering(&data, &schema, "", &mut ordering_errors);

        if !ordering_errors.is_empty() {
            bail!("property ordering errors:\n{}", ordering_errors.join("\n"));
        }

        Ok(())
    }
}

/// Recursively check that data object keys match the `propertyOrdering`
/// declared in the schema.
fn check_property_ordering(
    data: &Value,
    schema: &Value,
    path: &str,
    errors: &mut Vec<String>,
) {
    match (data, schema) {
        (Value::Object(data_map), Value::Object(schema_map)) => {
            // Check ordering at this level
            if let Some(Value::Array(expected_order)) = schema_map.get("propertyOrdering") {
                let expected: Vec<&str> = expected_order.iter()
                    .filter_map(|v| v.as_str())
                    .collect();

                let actual_keys: Vec<&str> = data_map.keys()
                    .map(|k| k.as_str())
                    .collect();

                // Filter actual keys to only those in the expected list
                let actual_ordered: Vec<&str> = actual_keys.iter()
                    .copied()
                    .filter(|k| expected.contains(k))
                    .collect();

                // Filter expected to only those present in data
                let expected_ordered: Vec<&str> = expected.iter()
                    .copied()
                    .filter(|k| actual_keys.contains(k))
                    .collect();

                if actual_ordered != expected_ordered {
                    let display_path = if path.is_empty() { "root" } else { path };
                    errors.push(format!(
                        "  {}: expected key order {:?}, got {:?}",
                        display_path, expected_ordered, actual_ordered,
                    ));
                }
            }

            // Recurse into properties
            if let Some(Value::Object(props)) = schema_map.get("properties") {
                for (key, prop_schema) in props {
                    if let Some(value) = data_map.get(key) {
                        let child_path = if path.is_empty() {
                            key.to_string()
                        } else {
                            format!("{}.{}", path, key)
                        };
                        check_property_ordering(value, prop_schema, &child_path, errors);
                    }
                }
            }

            // Recurse into items (for arrays-of-objects)
            if let Some(items_schema) = schema_map.get("items") {
                if let Value::Array(arr) = data {
                    for (i, item) in arr.iter().enumerate() {
                        let child_path = format!("{}[{}]", path, i);
                        check_property_ordering(item, items_schema, &child_path, errors);
                    }
                }
            }
        }
        (Value::Array(arr), schema_val) => {
            // Schema might have "items" at this level
            if let Some(items_schema) = schema_val.get("items") {
                for (i, item) in arr.iter().enumerate() {
                    let child_path = if path.is_empty() {
                        format!("[{}]", i)
                    } else {
                        format!("{}[{}]", path, i)
                    };
                    check_property_ordering(item, items_schema, &child_path, errors);
                }
            }
        }
        _ => {}
    }
}

impl_checker!(IyamlschemaProcessor,
    config: config,
    description: "Validate YAML files against JSON schemas (in-process)",
    name: crate::processors::names::IYAMLSCHEMA,
    execute: execute_product,
    config_json: true,
    native: true,
    batch: check_files,
);
```

- [ ] **Step 2: Add module declaration and pub use in checkers/mod.rs**

Add `mod iyamlschema;` in the module declarations section and `pub use iyamlschema::IyamlschemaProcessor;` in the pub use section.

- [ ] **Step 3: Re-export from processors/mod.rs**

Add `IyamlschemaProcessor` to the `pub use generators::{...}` line. Note: despite being a checker, it's re-exported from the same `pub use` line that lists all processor types.

Actually, checkers are re-exported separately. Find the line that re-exports checkers (has `IyamllintProcessor`) and add `IyamlschemaProcessor` there.

- [ ] **Step 4: Add registry entry in registry.rs**

Add one line in the `for_each_processor!` macro:

```rust
            IYAMLSCHEMA,    iyamlschema,    IyamlschemaConfig,     IyamlschemaProcessor,     ("", &[".yml", ".yaml"], BUILD_TOOL_EXCLUDES);
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo clean -p rsconstruct && cargo build`

- [ ] **Step 6: Commit**

```bash
git add src/processors/checkers/iyamlschema.rs src/processors/checkers/mod.rs \
        src/processors/mod.rs src/registry.rs
git commit -m "feat: add iyamlschema native processor"
```

---

### Task 6: Integration tests

**Files:**
- Create: `tests/processors/iyamlschema.rs`
- Modify: `tests/processors/mod.rs`

- [ ] **Step 1: Create test file**

The tests need a local HTTP server or pre-cached schemas. The simplest approach: use a file:// URL for the schema (the `ureq` crate supports file:// URLs, or we can put the schema file as an extra_input). Actually, the processor reads `$schema` as a URL and fetches it — for testing, create a schema file in the test project and reference it via a relative path that the webcache can handle. 

Alternatively, since the jsonschema crate's `resolve-file` feature is enabled, we can use `file://` URLs in tests.

```rust
// tests/processors/iyamlschema.rs
use std::fs;
use tempfile::TempDir;
use crate::common::run_rsconstruct_with_env;

fn write_schema(dir: &std::path::Path) -> String {
    let schema = r#"{
        "$schema": "http://json-schema.org/draft-07/schema#",
        "type": "object",
        "properties": {
            "name": { "type": "string" },
            "age": { "type": "integer" }
        },
        "propertyOrdering": ["name", "age"],
        "required": ["name"]
    }"#;
    let schema_path = dir.join("test_schema.json");
    fs::write(&schema_path, schema).unwrap();
    format!("file://{}", schema_path.display())
}

#[test]
fn iyamlschema_valid_file() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    let schema_url = write_schema(project_path);

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.iyamlschema]\nscan_dirs = [\".\"]\n",
    ).unwrap();

    fs::write(
        project_path.join("data.yaml"),
        format!("$schema: \"{}\"\nname: Alice\nage: 30\n", schema_url),
    ).unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build", "-v"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "Build should succeed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn iyamlschema_invalid_data_fails() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    let schema_url = write_schema(project_path);

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.iyamlschema]\nscan_dirs = [\".\"]\n",
    ).unwrap();

    // "age" should be integer, not string
    fs::write(
        project_path.join("data.yaml"),
        format!("$schema: \"{}\"\nname: Alice\nage: not_a_number\n", schema_url),
    ).unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(!output.status.success(), "Build should fail for invalid data");
}

#[test]
fn iyamlschema_wrong_ordering_fails() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    let schema_url = write_schema(project_path);

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.iyamlschema]\nscan_dirs = [\".\"]\n",
    ).unwrap();

    // Keys in wrong order: age before name
    fs::write(
        project_path.join("data.yaml"),
        format!("$schema: \"{}\"\nage: 30\nname: Alice\n", schema_url),
    ).unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(!output.status.success(), "Build should fail for wrong key order");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("property ordering"), "Error should mention property ordering: {}", stderr);
}

#[test]
fn iyamlschema_no_schema_field_fails() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.iyamlschema]\nscan_dirs = [\".\"]\n",
    ).unwrap();

    fs::write(
        project_path.join("data.yaml"),
        "name: Alice\nage: 30\n",
    ).unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(!output.status.success(), "Build should fail when $schema is missing");
}

#[test]
fn iyamlschema_incremental_skip() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    let schema_url = write_schema(project_path);

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.iyamlschema]\nscan_dirs = [\".\"]\n",
    ).unwrap();

    fs::write(
        project_path.join("data.yaml"),
        format!("$schema: \"{}\"\nname: Alice\nage: 30\n", schema_url),
    ).unwrap();

    // First build
    let output1 = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output1.status.success());

    // Second build should skip
    let output2 = run_rsconstruct_with_env(project_path, &["build", "--verbose"], &[("NO_COLOR", "1")]);
    assert!(output2.status.success());
    let stdout2 = String::from_utf8_lossy(&output2.stdout);
    assert!(
        stdout2.contains("[iyamlschema] Skipping (unchanged):"),
        "Second build should skip: {}", stdout2,
    );
}
```

- [ ] **Step 2: Add module declaration**

In `tests/processors/mod.rs`, add:
```rust
mod iyamlschema;
```

- [ ] **Step 3: Run the tests**

Run: `cargo test iyamlschema`
Expected: all 5 tests pass

- [ ] **Step 4: Run full test suite**

Run: `cargo test`
Expected: no regressions (only pre-existing `cc_single_file_angle_bracket_include_dependency` may fail)

- [ ] **Step 5: Commit**

```bash
git add tests/processors/iyamlschema.rs tests/processors/mod.rs
git commit -m "test: add iyamlschema integration tests"
```

---

### Task 7: Update data project config

**Files:**
- Modify: `../data/rsconstruct.toml`

- [ ] **Step 1: Replace script.validate_yaml with iyamlschema**

Replace:
```toml
[processor.script.validate_yaml]
scan_dirs = ["yaml"]
extensions = [".yaml"]
linter = "scripts/validate_yaml.py"
batch = true
```

With:
```toml
[processor.iyamlschema]
scan_dirs = ["yaml"]
```

- [ ] **Step 2: Verify it discovers files**

Run from `../data`:
```bash
/path/to/rsconstruct processors files --headers | grep iyamlschema
```
Expected: shows 21 YAML files under `[iyamlschema]`

- [ ] **Step 3: Commit**

```bash
git add rsconstruct.toml
git commit -m "config: use native iyamlschema instead of validate_yaml.py script"
```

---

## Notes

- The `ureq` crate is a blocking HTTP client — this is intentional. The processor executes in a sync context (the `execute` method is not async). Schema fetches are cached after the first call, so network latency only affects the first build.
- The `file://` URL scheme in tests avoids requiring network access during CI.
- The `propertyOrdering` check mirrors the logic in `scripts/validate_yaml.py` — it checks that data keys appear in the order specified by the schema's `propertyOrdering` array.
- The webcache uses the same `.rsconstruct/` directory as the build cache but in a separate `webcache/` subdirectory.
- The `cache clear` command already deletes the entire `.rsconstruct/` directory, which will also clear the webcache. The `webcache clear` command only clears the webcache subdirectory.
