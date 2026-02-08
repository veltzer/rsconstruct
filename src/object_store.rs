use anyhow::{Context, Result};
use sha2::{Sha256, Digest};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use serde::{Deserialize, Serialize};
use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};

use crate::color;
use crate::config::RestoreMethod;
use crate::remote_cache::RemoteCache;

/// Number of hex chars used as the subdirectory prefix for object storage (git-style sharding).
const CHECKSUM_PREFIX_LEN: usize = 2;

/// Recursively collect all files under a directory.
fn walk_files(dir: &Path) -> Vec<PathBuf> {
    let mut result = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                result.extend(walk_files(&path));
            } else if path.is_file() {
                result.push(path);
            }
        }
    }
    result
}

const RSB_DIR: &str = ".rsb";
const OBJECTS_DIR: &str = "objects";
const DB_FILE: &str = "db.redb";

const CACHE_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("cache");
const CONFIGS_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("processor_configs");
const MTIME_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("mtime_cache");

/// Cached mtime-to-checksum mapping for a single file
#[derive(Serialize, Deserialize)]
struct MtimeEntry {
    mtime_secs: i64,
    mtime_nanos: u32,
    checksum: String,
}

/// Reason why a product needs to be rebuilt.
#[derive(Debug)]
pub enum RebuildReason {
    /// No cache entry exists for this product
    NoCacheEntry,
    /// Input files have changed since last build
    InputsChanged,
    /// An output file is missing (and can't be restored from cache)
    OutputMissing(String),
    /// Build was forced with --force flag
    Force,
}

impl std::fmt::Display for RebuildReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RebuildReason::NoCacheEntry => write!(f, "no cache entry"),
            RebuildReason::InputsChanged => write!(f, "inputs changed"),
            RebuildReason::OutputMissing(path) => write!(f, "output missing: {}", path),
            RebuildReason::Force => write!(f, "forced"),
        }
    }
}

/// The action the executor will take for a product, with an explanation.
#[derive(Debug)]
pub enum ExplainAction {
    /// Product is up-to-date, will be skipped
    Skip,
    /// Product will be restored from cache
    Restore(RebuildReason),
    /// Product will be rebuilt
    Rebuild(RebuildReason),
}

impl std::fmt::Display for ExplainAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExplainAction::Skip => write!(f, "SKIP (inputs unchanged)"),
            ExplainAction::Restore(reason) => write!(f, "RESTORE ({})", reason),
            ExplainAction::Rebuild(reason) => write!(f, "BUILD ({})", reason),
        }
    }
}

/// Object store for caching build outputs
/// Uses git-like object storage: .rsb/objects/[2 chars]/[rest of hash]
/// Index is stored in a redb embedded key/value database at .rsb/db.redb
pub struct ObjectStore {
    /// Path to .rsb directory
    rsb_dir: PathBuf,
    /// Path to objects directory
    objects_dir: PathBuf,
    /// redb database for cache index
    db: Database,
    /// Method to restore files from cache
    restore_method: RestoreMethod,
    /// Optional remote cache backend
    remote: Option<Box<dyn RemoteCache>>,
    /// Whether to push to remote cache
    remote_push: bool,
    /// Whether to pull from remote cache
    remote_pull: bool,
    /// Whether to use mtime pre-check to skip unchanged file checksums
    mtime_check: bool,
}

/// Information about a cached product
#[derive(Debug, Serialize, Deserialize, Clone)]
struct CacheEntry {
    /// Combined checksum of all inputs at time of caching
    input_checksum: String,
    /// List of output files and their checksums
    outputs: Vec<OutputEntry>,
}

/// Information about a single cached output file
#[derive(Debug, Serialize, Deserialize, Clone)]
struct OutputEntry {
    /// Original path of the output file (relative to project root)
    path: String,
    /// Checksum of the output content (used as object store key)
    checksum: String,
}

/// Per-processor cache statistics
#[derive(Debug, Default, Serialize)]
pub struct ProcessorCacheStats {
    pub entry_count: usize,
    pub output_count: usize,
    pub output_bytes: u64,
}

/// Information about a cache entry for display
#[derive(Serialize)]
pub struct CacheListEntry {
    pub cache_key: String,
    pub input_checksum: String,
    /// Output paths and whether the object exists in the store
    pub outputs: Vec<CacheListOutput>,
}

/// Information about a single output in a cache list entry
#[derive(Serialize)]
pub struct CacheListOutput {
    pub path: String,
    pub exists: bool,
}

/// Options for configuring an ObjectStore instance.
pub struct ObjectStoreOptions {
    pub restore_method: RestoreMethod,
    pub remote: Option<Box<dyn RemoteCache>>,
    pub remote_push: bool,
    pub remote_pull: bool,
    pub mtime_check: bool,
}

impl ObjectStore {
    pub fn new(opts: ObjectStoreOptions) -> Result<Self> {
        let rsb_dir = PathBuf::from(RSB_DIR);
        let objects_dir = rsb_dir.join(OBJECTS_DIR);
        let db_path = rsb_dir.join(DB_FILE);

        // Ensure .rsb directory exists
        fs::create_dir_all(&rsb_dir)
            .context("Failed to create .rsb directory")?;

        let db = crate::db::open_or_recreate(&db_path, "Cache database")?;

        Ok(Self {
            rsb_dir,
            objects_dir,
            db,
            restore_method: opts.restore_method,
            remote: opts.remote,
            remote_push: opts.remote_push,
            remote_pull: opts.remote_pull,
            mtime_check: opts.mtime_check,
        })
    }

    /// Set whether mtime pre-check is enabled.
    pub fn set_mtime_check(&mut self, enabled: bool) {
        self.mtime_check = enabled;
    }

    /// Get a cache entry from the database
    fn get_entry(&self, cache_key: &str) -> Option<CacheEntry> {
        let read_txn = self.db.begin_read().ok()?;
        let table = read_txn.open_table(CACHE_TABLE).ok()?;
        let data = table.get(cache_key).ok()??;
        serde_json::from_slice(data.value()).ok()
    }

    /// Insert a cache entry into the database
    fn insert_entry(&self, cache_key: &str, entry: &CacheEntry) -> Result<()> {
        let value = serde_json::to_vec(entry)
            .context("Failed to serialize cache entry")?;
        let write_txn = self.db.begin_write()
            .context("Failed to begin write transaction")?;
        {
            let mut table = write_txn.open_table(CACHE_TABLE)
                .context("Failed to open cache table")?;
            table.insert(cache_key, value.as_slice())
                .context("Failed to insert cache entry")?;
        }
        write_txn.commit()
            .context("Failed to commit cache entry")?;
        Ok(())
    }

    /// Calculate SHA-256 checksum of a file
    pub fn calculate_checksum(file_path: &Path) -> Result<String> {
        let contents = fs::read(file_path)
            .with_context(|| format!("Failed to read file for checksum: {}", file_path.display()))?;
        let mut hasher = Sha256::new();
        hasher.update(&contents);
        let result = hasher.finalize();
        Ok(hex::encode(result))
    }

    /// Calculate SHA-256 checksum of bytes
    pub fn calculate_checksum_bytes(data: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(data);
        let result = hasher.finalize();
        hex::encode(result)
    }

    /// Get object path for a checksum (e.g., .rsb/objects/ab/cdef123...)
    fn object_path(&self, checksum: &str) -> PathBuf {
        let (prefix, rest) = checksum.split_at(CHECKSUM_PREFIX_LEN.min(checksum.len()));
        self.objects_dir.join(prefix).join(rest)
    }

    /// Store content in object store, returns checksum
    fn store_object(&self, content: &[u8]) -> Result<String> {
        let checksum = Self::calculate_checksum_bytes(content);
        let object_path = self.object_path(&checksum);

        // Only write if not already stored
        if !object_path.exists() {
            if let Some(parent) = object_path.parent() {
                fs::create_dir_all(parent)
                    .context("Failed to create object directory")?;
            }
            fs::write(&object_path, content)
                .context("Failed to write object")?;
        }

        Ok(checksum)
    }

    /// Check if an object exists in the store
    fn has_object(&self, checksum: &str) -> bool {
        self.object_path(checksum).exists()
    }

    /// Restore a file from the object store using configured method
    fn restore_file(&self, checksum: &str, output_path: &Path) -> Result<()> {
        let object_path = self.object_path(checksum);

        match self.restore_method {
            RestoreMethod::Hardlink => {
                fs::hard_link(&object_path, output_path)
                    .with_context(|| format!("Failed to hard link from cache: {}. If on a cross-filesystem setup, set restore_method = \"copy\" in rsb.toml.", checksum))?;
            }
            RestoreMethod::Copy => {
                fs::copy(&object_path, output_path)
                    .with_context(|| format!("Failed to copy from cache: {}", checksum))?;
            }
        }

        Ok(())
    }

    /// Check if a product needs rebuilding
    /// Returns true if inputs changed or outputs are missing
    pub fn needs_rebuild(&self, cache_key: &str, input_checksum: &str, output_paths: &[PathBuf]) -> bool {
        // Check if we have a cache entry
        let entry = match self.get_entry(cache_key) {
            Some(e) => e,
            None => return true,
        };

        // Check if input checksum matches
        if entry.input_checksum != input_checksum {
            return true;
        }

        // For checkers (empty outputs), cache entry with matching checksum = up-to-date
        if output_paths.is_empty() {
            return false;
        }

        // Check if all outputs exist at their original paths
        for output_path in output_paths {
            if !output_path.exists() {
                // Output missing - check if we can restore from cache
                let rel_path = Self::path_string(output_path);
                let cached_output = entry.outputs.iter()
                    .find(|o| o.path == rel_path);

                match cached_output {
                    Some(out) if self.has_object(&out.checksum) => {
                        // Can restore from cache, but still "needs rebuild" to trigger restore
                        return true;
                    }
                    _ => return true,
                }
            }
        }

        false
    }

    /// Check if outputs can be restored from cache (read-only, does not restore)
    /// Returns true if all missing outputs are available in cache
    pub fn can_restore(&self, cache_key: &str, input_checksum: &str, output_paths: &[PathBuf]) -> bool {
        // For checkers (empty outputs), cache entry with matching checksum = restorable
        if output_paths.is_empty() {
            return self.get_entry(cache_key)
                .map(|e| e.input_checksum == input_checksum)
                .unwrap_or(false);
        }

        let entry = match self.get_entry(cache_key) {
            Some(e) if e.input_checksum == input_checksum => e,
            _ => return false,
        };

        for output_path in output_paths {
            if output_path.exists() {
                continue;
            }

            let rel_path = Self::path_string(output_path);
            let cached_output = entry.outputs.iter()
                .find(|o| o.path == rel_path);

            match cached_output {
                Some(out) if self.has_object(&out.checksum) => {}
                _ => return false,
            }
        }

        true
    }

    /// Explain what action will be taken for a product and why.
    /// Mirrors the logic in needs_rebuild/can_restore but returns structured reasons.
    pub fn explain_action(&self, cache_key: &str, input_checksum: &str, output_paths: &[PathBuf], force: bool) -> ExplainAction {
        if force {
            return ExplainAction::Rebuild(RebuildReason::Force);
        }

        let entry = match self.get_entry(cache_key) {
            Some(e) => e,
            None => return ExplainAction::Rebuild(RebuildReason::NoCacheEntry),
        };

        if entry.input_checksum != input_checksum {
            // Inputs changed — check if restorable (shouldn't be, since checksum differs)
            return ExplainAction::Rebuild(RebuildReason::InputsChanged);
        }

        // For checkers (empty outputs), matching checksum means up-to-date
        if output_paths.is_empty() {
            return ExplainAction::Skip;
        }

        // Check outputs
        for output_path in output_paths {
            if !output_path.exists() {
                let rel_path = Self::path_string(output_path);
                let cached_output = entry.outputs.iter().find(|o| o.path == rel_path);
                match cached_output {
                    Some(out) if self.has_object(&out.checksum) => {
                        return ExplainAction::Restore(RebuildReason::OutputMissing(rel_path));
                    }
                    _ => {
                        return ExplainAction::Rebuild(RebuildReason::OutputMissing(rel_path));
                    }
                }
            }
        }

        ExplainAction::Skip
    }

    /// Try to restore outputs from cache (local first, then remote).
    /// Returns true if all outputs were restored (or for checkers, if cache entry is valid).
    ///
    /// # Behavior by processor type
    ///
    /// - **Generators** (non-empty `output_paths`): Restores output files from cached objects
    ///   via hardlink or copy. Returns true only if all outputs were successfully restored.
    ///
    /// - **Checkers** (empty `output_paths`): No files to restore. Returns true if a cache
    ///   entry exists with matching input checksum, indicating the check previously passed.
    ///   This allows checkers to skip re-running after `rsb clean && rsb build`.
    pub fn restore_from_cache(&self, cache_key: &str, input_checksum: &str, output_paths: &[PathBuf]) -> Result<bool> {
        // For checkers (empty outputs), just verify cache entry exists with matching checksum.
        // The cache entry itself serves as the "success marker" - no files need restoration.
        if output_paths.is_empty() {
            return Ok(self.get_entry(cache_key)
                .map(|e| e.input_checksum == input_checksum)
                .unwrap_or(false));
        }

        // Check if we have a cache entry with matching input checksum
        let entry = match self.get_entry(cache_key) {
            Some(e) if e.input_checksum == input_checksum => Some(e),
            _ => {
                // Try to fetch from remote if enabled
                if self.remote_pull {
                    self.try_fetch_from_remote(cache_key, input_checksum)?
                } else {
                    None
                }
            }
        };

        let entry = match entry {
            Some(e) => e,
            None => return Ok(false),
        };

        // Verify input checksum matches
        if entry.input_checksum != input_checksum {
            return Ok(false);
        }

        // Try to restore each missing output
        for output_path in output_paths {
            if output_path.exists() {
                continue;
            }

            let rel_path = Self::path_string(output_path);
            let cached_output = entry.outputs.iter()
                .find(|o| o.path == rel_path);

            match cached_output {
                Some(out) => {
                    // Check local cache first
                    if self.has_object(&out.checksum) {
                        if let Some(parent) = output_path.parent() {
                            fs::create_dir_all(parent)?;
                        }
                        self.restore_file(&out.checksum, output_path)?;
                    } else if self.remote_pull {
                        // Try to fetch object from remote
                        if !self.try_fetch_object_from_remote(&out.checksum)? {
                            return Ok(false);
                        }
                        if let Some(parent) = output_path.parent() {
                            fs::create_dir_all(parent)?;
                        }
                        self.restore_file(&out.checksum, output_path)?;
                    } else {
                        return Ok(false);
                    }
                }
                None => return Ok(false),
            }
        }

        Ok(true)
    }

    /// Try to fetch a cache entry from remote cache
    fn try_fetch_from_remote(&self, cache_key: &str, input_checksum: &str) -> Result<Option<CacheEntry>> {
        let remote = match &self.remote {
            Some(r) => r,
            None => return Ok(None),
        };

        let remote_key = format!("index/{}", cache_key);
        let data = match remote.download_bytes(&remote_key)? {
            Some(d) => d,
            None => return Ok(None),
        };

        let entry: CacheEntry = match serde_json::from_slice(&data) {
            Ok(e) => e,
            Err(_) => return Ok(None),
        };

        // Verify the input checksum matches what we expect
        if entry.input_checksum != input_checksum {
            return Ok(None);
        }

        // Store the entry locally for future use
        self.insert_entry(cache_key, &entry)?;

        Ok(Some(entry))
    }

    /// Try to fetch an object from remote cache
    fn try_fetch_object_from_remote(&self, checksum: &str) -> Result<bool> {
        let remote = match &self.remote {
            Some(r) => r,
            None => return Ok(false),
        };

        let object_path = self.object_path(checksum);
        if object_path.exists() {
            return Ok(true);
        }

        let remote_key = format!("objects/{}/{}", &checksum[..2], &checksum[2..]);
        if remote.download(&remote_key, &object_path)? {
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Cache the outputs of a successful build.
    /// Returns `Ok(true)` if any output content changed compared to the previous
    /// cache entry, `Ok(false)` if all outputs are content-identical.
    pub fn cache_outputs(&self, cache_key: &str, input_checksum: &str, output_paths: &[PathBuf]) -> Result<bool> {
        // Get previous entry for comparison
        let prev_entry = self.get_entry(cache_key);

        let mut outputs = Vec::new();
        let mut any_changed = false;

        for output_path in output_paths {
            if !output_path.exists() {
                continue;
            }

            let content = fs::read(output_path)?;
            let checksum = self.store_object(&content)?;
            let rel_path = Self::path_string(output_path);

            // Check if this output changed vs previous entry
            if !any_changed {
                let prev_checksum = prev_entry.as_ref().and_then(|e| {
                    e.outputs.iter().find(|o| o.path == rel_path).map(|o| &o.checksum)
                });
                if prev_checksum != Some(&checksum) {
                    any_changed = true;
                }
            }

            // Push object to remote cache if enabled
            if self.remote_push {
                self.try_push_object_to_remote(&checksum)?;
            }

            outputs.push(OutputEntry {
                path: rel_path,
                checksum,
            });
        }

        // For checkers (empty outputs), nothing changed
        if output_paths.is_empty() {
            any_changed = false;
        }

        // Check if number of outputs changed
        if !any_changed {
            if let Some(ref prev) = prev_entry {
                if prev.outputs.len() != outputs.len() {
                    any_changed = true;
                }
            } else {
                // No previous entry means this is new
                any_changed = true;
            }
        }

        let entry = CacheEntry {
            input_checksum: input_checksum.to_string(),
            outputs,
        };

        self.insert_entry(cache_key, &entry)?;

        // Push index entry to remote cache if enabled
        if self.remote_push {
            self.try_push_entry_to_remote(cache_key, &entry)?;
        }

        Ok(any_changed)
    }

    /// Try to push an object to remote cache (ignores errors)
    fn try_push_object_to_remote(&self, checksum: &str) -> Result<()> {
        let remote = match &self.remote {
            Some(r) => r,
            None => return Ok(()),
        };

        let object_path = self.object_path(checksum);
        if !object_path.exists() {
            return Ok(());
        }

        let remote_key = format!("objects/{}/{}", &checksum[..2], &checksum[2..]);

        // Check if already exists remotely (avoid redundant uploads)
        if remote.exists(&remote_key).unwrap_or(false) {
            return Ok(());
        }

        // Upload (ignore errors - remote cache is best-effort)
        if let Err(e) = remote.upload(&remote_key, &object_path) {
            eprintln!("Warning: failed to push to remote cache: {}", e);
        }

        Ok(())
    }

    /// Try to push a cache entry to remote cache (ignores errors)
    fn try_push_entry_to_remote(&self, cache_key: &str, entry: &CacheEntry) -> Result<()> {
        let remote = match &self.remote {
            Some(r) => r,
            None => return Ok(()),
        };

        let remote_key = format!("index/{}", cache_key);
        let data = serde_json::to_vec(entry)
            .context("Failed to serialize cache entry for remote")?;

        // Upload (ignore errors - remote cache is best-effort)
        if let Err(e) = remote.upload_bytes(&remote_key, &data) {
            eprintln!("Warning: failed to push index to remote cache: {}", e);
        }

        Ok(())
    }

    /// Store a processor's config JSON for later comparison.
    /// Returns the previous config if it existed and was different.
    pub fn store_processor_config(&self, processor: &str, config_json: &str) -> Result<Option<String>> {
        // Read old value
        let old_value = {
            let read_txn = self.db.begin_read()
                .context("Failed to begin read transaction")?;
            match read_txn.open_table(CONFIGS_TABLE) {
                Ok(table) => {
                    table.get(processor).ok()
                        .flatten()
                        .and_then(|bytes| String::from_utf8(bytes.value().to_vec()).ok())
                }
                Err(_) => None,
            }
        };

        // Only update if changed
        let changed = old_value.as_ref() != Some(&config_json.to_string());
        if changed {
            let write_txn = self.db.begin_write()
                .context("Failed to begin write transaction")?;
            {
                let mut table = write_txn.open_table(CONFIGS_TABLE)
                    .context("Failed to open configs table")?;
                table.insert(processor, config_json.as_bytes())
                    .context("Failed to store processor config")?;
            }
            write_txn.commit()
                .context("Failed to commit processor config")?;
        }

        // Return old value only if it was different
        if changed {
            Ok(old_value)
        } else {
            Ok(None)
        }
    }

    /// Generate a colored diff between old and new config JSON.
    /// Returns None if configs are identical or if diffing fails.
    pub fn diff_configs(old_json: &str, new_json: &str) -> Option<String> {
        // Parse both as generic JSON values
        let old: serde_json::Value = serde_json::from_str(old_json).ok()?;
        let new: serde_json::Value = serde_json::from_str(new_json).ok()?;

        if old == new {
            return None;
        }

        // Convert to sorted maps for comparison
        let old_map = Self::flatten_json(&old, "");
        let new_map = Self::flatten_json(&new, "");

        let mut lines: Vec<String> = Vec::new();

        // Find removed and changed keys
        for (key, old_val) in &old_map {
            match new_map.get(key) {
                None => {
                    let s = format!("- {}: {}", key, old_val);
                    lines.push(color::red(&s).into_owned());
                }
                Some(new_val) if new_val != old_val => {
                    let old_s = format!("- {}: {}", key, old_val);
                    lines.push(color::red(&old_s).into_owned());
                    let new_s = format!("+ {}: {}", key, new_val);
                    lines.push(color::green(&new_s).into_owned());
                }
                _ => {}
            }
        }

        // Find added keys
        for (key, new_val) in &new_map {
            if !old_map.contains_key(key) {
                let s = format!("+ {}: {}", key, new_val);
                lines.push(color::green(&s).into_owned());
            }
        }

        if lines.is_empty() {
            None
        } else {
            Some(lines.join("\n"))
        }
    }

    /// Flatten a JSON value into a map of dotted keys to string values
    fn flatten_json(value: &serde_json::Value, prefix: &str) -> BTreeMap<String, String> {
        let mut map = BTreeMap::new();

        match value {
            serde_json::Value::Object(obj) => {
                for (k, v) in obj {
                    let key = if prefix.is_empty() {
                        k.clone()
                    } else {
                        format!("{}.{}", prefix, k)
                    };
                    map.extend(Self::flatten_json(v, &key));
                }
            }
            serde_json::Value::Array(arr) => {
                for (i, v) in arr.iter().enumerate() {
                    let key = format!("{}[{}]", prefix, i);
                    map.extend(Self::flatten_json(v, &key));
                }
            }
            _ => {
                let val_str = match value {
                    serde_json::Value::String(s) => format!("\"{}\"", s),
                    serde_json::Value::Null => "null".to_string(),
                    v => v.to_string(),
                };
                map.insert(prefix.to_string(), val_str);
            }
        }

        map
    }

    /// Convert path to string for storage. Paths are already relative.
    fn path_string(path: &Path) -> String {
        path.display().to_string()
    }

    /// Clear the entire cache
    pub fn clear(&mut self) -> Result<()> {
        // Drop the database before removing the directory.
        // Create a temporary database to replace the current one
        let temp_dir = std::env::temp_dir().join(format!("rsb_temp_{}", std::process::id()));
        fs::create_dir_all(&temp_dir)?;
        let temp_db_path = temp_dir.join("temp.redb");
        let temp_db = Database::create(&temp_db_path)
            .context("Failed to create temporary database")?;
        self.db = temp_db;

        if self.rsb_dir.exists() {
            fs::remove_dir_all(&self.rsb_dir)
                .context("Failed to remove .rsb directory")?;
        }

        // Clean up temp dir
        let _ = fs::remove_dir_all(&temp_dir);

        Ok(())
    }

    /// Get cache size in bytes and number of objects
    pub fn size(&self) -> Result<(u64, usize)> {
        let mut total_bytes = 0u64;
        let mut object_count = 0usize;

        if !self.objects_dir.exists() {
            return Ok((0, 0));
        }

        for path in walk_files(&self.objects_dir) {
            if let Ok(metadata) = fs::metadata(&path) {
                total_bytes += metadata.len();
                object_count += 1;
            }
        }

        Ok((total_bytes, object_count))
    }

    /// Trim cache by removing objects not referenced in the index
    pub fn trim(&self) -> Result<(u64, usize)> {
        let mut removed_bytes = 0u64;
        let mut removed_count = 0usize;

        if !self.objects_dir.exists() {
            return Ok((0, 0));
        }

        // Collect all referenced checksums
        let mut referenced: std::collections::HashSet<String> = std::collections::HashSet::new();
        {
            let read_txn = self.db.begin_read()
                .context("Failed to begin read transaction for trim")?;
            if let Ok(table) = read_txn.open_table(CACHE_TABLE)
                && let Ok(iter) = table.iter() {
                    for result in iter {
                        let (_, value) = result.context("Failed to read cache entry during trim")?;
                        if let Ok(entry) = serde_json::from_slice::<CacheEntry>(value.value()) {
                            for output in &entry.outputs {
                                referenced.insert(output.checksum.clone());
                            }
                        }
                    }
                }
        }

        // Find and remove unreferenced objects
        let mut to_remove = Vec::new();
        for path in walk_files(&self.objects_dir) {
            // Reconstruct checksum from path
            if let (Some(prefix), Some(rest)) = (
                path.parent().and_then(|p| p.file_name()).and_then(|n| n.to_str()),
                path.file_name().and_then(|n| n.to_str())
            ) {
                let checksum = format!("{}{}", prefix, rest);
                if !referenced.contains(&checksum) {
                    if let Ok(metadata) = fs::metadata(&path) {
                        removed_bytes += metadata.len();
                        removed_count += 1;
                    }
                    to_remove.push(path);
                }
            }
        }

        // Remove unreferenced objects
        for path in to_remove {
            fs::remove_file(&path)?;
            // Try to remove empty parent directory
            if let Some(parent) = path.parent() {
                let _ = fs::remove_dir(parent); // Ignore error if not empty
            }
        }

        Ok((removed_bytes, removed_count))
    }

    /// Remove stale index entries whose cache keys are not in the valid set.
    /// Returns the number of entries removed.
    pub fn remove_stale(&self, valid_keys: &std::collections::HashSet<String>) -> usize {
        let mut count = 0;

        // First, collect stale keys
        let stale_keys: Vec<String> = {
            let read_txn = match self.db.begin_read() {
                Ok(t) => t,
                Err(_) => return 0,
            };
            let table = match read_txn.open_table(CACHE_TABLE) {
                Ok(t) => t,
                Err(_) => return 0,
            };
            let iter = match table.iter() {
                Ok(i) => i,
                Err(_) => return 0,
            };
            iter.filter_map(|result| {
                let (key, _) = result.ok()?;
                let key_str = key.value().to_string();
                if !valid_keys.contains(&key_str) {
                    Some(key_str)
                } else {
                    None
                }
            })
            .collect()
        };

        // Then remove them in a write transaction
        if !stale_keys.is_empty()
            && let Ok(write_txn) = self.db.begin_write() {
                if let Ok(mut table) = write_txn.open_table(CACHE_TABLE) {
                    for key in &stale_keys {
                        if table.remove(key.as_str()).is_ok() {
                            count += 1;
                        }
                    }
                }
                let _ = write_txn.commit();
            }

        count
    }

    /// List all cache entries with their status
    pub fn list(&self) -> Vec<CacheListEntry> {
        let read_txn = match self.db.begin_read() {
            Ok(t) => t,
            Err(_) => return Vec::new(),
        };
        let table = match read_txn.open_table(CACHE_TABLE) {
            Ok(t) => t,
            Err(_) => return Vec::new(),
        };
        let iter = match table.iter() {
            Ok(i) => i,
            Err(_) => return Vec::new(),
        };
        let mut entries: Vec<CacheListEntry> = iter
            .filter_map(|result| {
                let (key, value) = result.ok()?;
                let key_str = key.value().to_string();
                let entry: CacheEntry = serde_json::from_slice(value.value()).ok()?;
                let outputs = entry.outputs.iter().map(|o| {
                    CacheListOutput {
                        path: o.path.clone(),
                        exists: self.has_object(&o.checksum),
                    }
                }).collect();
                Some(CacheListEntry {
                    cache_key: key_str,
                    input_checksum: entry.input_checksum,
                    outputs,
                })
            })
            .collect();
        entries.sort_by(|a, b| a.cache_key.cmp(&b.cache_key));
        entries
    }

    /// Get per-processor cache statistics.
    /// Extracts the processor name from the cache key prefix (before first ":").
    pub fn stats_by_processor(&self) -> BTreeMap<String, ProcessorCacheStats> {
        let mut stats: BTreeMap<String, ProcessorCacheStats> = BTreeMap::new();

        let read_txn = match self.db.begin_read() {
            Ok(t) => t,
            Err(_) => return stats,
        };
        let table = match read_txn.open_table(CACHE_TABLE) {
            Ok(t) => t,
            Err(_) => return stats,
        };
        let iter = match table.iter() {
            Ok(i) => i,
            Err(_) => return stats,
        };

        for result in iter {
            let (key, value) = match result {
                Ok(kv) => kv,
                Err(_) => continue,
            };
            let key_str = key.value().to_string();
            let processor = key_str.split(':').next().unwrap_or(&key_str).to_string();

            let proc_stats = stats.entry(processor).or_default();
            proc_stats.entry_count += 1;

            if let Ok(entry) = serde_json::from_slice::<CacheEntry>(value.value()) {
                proc_stats.output_count += entry.outputs.len();
                for output in &entry.outputs {
                    let obj_path = self.object_path(&output.checksum);
                    if let Ok(metadata) = fs::metadata(&obj_path) {
                        proc_stats.output_bytes += metadata.len();
                    }
                }
            }
        }

        stats
    }

    /// Get the checksum for a file, using mtime to avoid re-reading unchanged files.
    /// If the file's mtime matches the cached entry, returns the cached checksum.
    /// Otherwise reads the file, computes SHA-256, and caches the result.
    fn fast_checksum(&self, file_path: &Path) -> Result<(String, Option<(String, MtimeEntry)>)> {
        let metadata = fs::metadata(file_path)
            .with_context(|| format!("Failed to stat file: {}", file_path.display()))?;
        let mtime = metadata.modified()
            .with_context(|| format!("Failed to get mtime: {}", file_path.display()))?;
        let duration = mtime.duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default();
        let mtime_secs = duration.as_secs() as i64;
        let mtime_nanos = duration.subsec_nanos();

        let path_str = file_path.display().to_string();

        // Check mtime cache
        let cached = {
            let read_txn = self.db.begin_read()
                .context("Failed to begin read transaction for mtime cache")?;
            match read_txn.open_table(MTIME_TABLE) {
                Ok(table) => {
                    table.get(path_str.as_str()).ok()
                        .flatten()
                        .and_then(|data| serde_json::from_slice::<MtimeEntry>(data.value()).ok())
                }
                Err(_) => None,
            }
        };

        if let Some(ref entry) = cached
            && entry.mtime_secs == mtime_secs && entry.mtime_nanos == mtime_nanos {
                return Ok((entry.checksum.clone(), None));
            }

        // mtime changed or no cache entry — compute checksum
        let checksum = Self::calculate_checksum(file_path)?;
        let new_entry = MtimeEntry {
            mtime_secs,
            mtime_nanos,
            checksum: checksum.clone(),
        };

        Ok((checksum, Some((path_str, new_entry))))
    }

    /// Flush a batch of dirty mtime entries in a single write transaction.
    fn flush_mtime_entries(&self, dirty: Vec<(String, MtimeEntry)>) -> Result<()> {
        if dirty.is_empty() {
            return Ok(());
        }
        let write_txn = self.db.begin_write()
            .context("Failed to begin write transaction for mtime cache")?;
        {
            let mut table = write_txn.open_table(MTIME_TABLE)
                .context("Failed to open mtime cache table")?;
            for (path_str, entry) in &dirty {
                let value = serde_json::to_vec(entry)
                    .context("Failed to serialize mtime entry")?;
                table.insert(path_str.as_str(), value.as_slice())
                    .context("Failed to insert mtime entry")?;
            }
        }
        write_txn.commit()
            .context("Failed to commit mtime cache entries")?;
        Ok(())
    }

    /// Get the combined input checksum using mtime-based caching.
    /// Same semantics as `combined_input_checksum()` but avoids re-reading
    /// unchanged files by checking file modification times first.
    /// Falls back to full checksums when mtime_check is disabled.
    pub fn combined_input_checksum_fast(&self, inputs: &[PathBuf]) -> Result<String> {
        if !self.mtime_check {
            return Self::combined_input_checksum(inputs);
        }

        let mut checksums = Vec::new();
        let mut dirty_entries = Vec::new();

        for input in inputs {
            if input.exists() {
                let (checksum, dirty) = self.fast_checksum(input)?;
                checksums.push(checksum);
                if let Some(entry) = dirty {
                    dirty_entries.push(entry);
                }
            } else {
                checksums.push(format!("MISSING:{}", input.display()));
            }
        }

        // Flush all dirty mtime entries in a single transaction
        self.flush_mtime_entries(dirty_entries)?;

        Ok(checksums.join(":"))
    }

    /// Get the cached input checksum for a product from its cache entry.
    pub fn get_cached_input_checksum(&self, cache_key: &str) -> Option<String> {
        self.get_entry(cache_key).map(|e| e.input_checksum)
    }

    /// Get the combined input checksum for a list of input files.
    /// Missing files are represented by a sentinel so that different sets of
    /// missing files never collide.
    pub fn combined_input_checksum(inputs: &[PathBuf]) -> Result<String> {
        let mut checksums = Vec::new();
        for input in inputs {
            if input.exists() {
                checksums.push(Self::calculate_checksum(input)?);
            } else {
                checksums.push(format!("MISSING:{}", input.display()));
            }
        }
        Ok(checksums.join(":"))
    }
}
