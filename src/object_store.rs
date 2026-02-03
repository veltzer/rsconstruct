use anyhow::{Context, Result};
use sha2::{Sha256, Digest};
use std::fs;
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};

use crate::config::RestoreMethod;
use crate::remote_cache::RemoteCache;

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
const DB_DIR: &str = "db";

/// Object store for caching build outputs
/// Uses git-like object storage: .rsb/objects/[2 chars]/[rest of hash]
/// Index is stored in a sled embedded key/value database at .rsb/db/
pub struct ObjectStore {
    /// Path to .rsb directory
    rsb_dir: PathBuf,
    /// Path to objects directory
    objects_dir: PathBuf,
    /// sled database for cache index
    db: sled::Db,
    /// Method to restore files from cache
    restore_method: RestoreMethod,
    /// Optional remote cache backend
    remote: Option<Box<dyn RemoteCache>>,
    /// Whether to push to remote cache
    remote_push: bool,
    /// Whether to pull from remote cache
    remote_pull: bool,
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

impl ObjectStore {
    pub fn new(
        restore_method: RestoreMethod,
        remote: Option<Box<dyn RemoteCache>>,
        remote_push: bool,
        remote_pull: bool,
    ) -> Result<Self> {
        let rsb_dir = PathBuf::from(RSB_DIR);
        let objects_dir = rsb_dir.join(OBJECTS_DIR);
        let db_path = rsb_dir.join(DB_DIR);

        // Ensure .rsb directory exists
        fs::create_dir_all(&rsb_dir)
            .context("Failed to create .rsb directory")?;

        // Open sled database
        let db = sled::open(&db_path)
            .context("Failed to open cache database")?;

        Ok(Self {
            rsb_dir,
            objects_dir,
            db,
            restore_method,
            remote,
            remote_push,
            remote_pull,
        })
    }

    /// Get a cache entry from the database
    fn get_entry(&self, cache_key: &str) -> Option<CacheEntry> {
        self.db.get(cache_key.as_bytes()).ok().flatten().and_then(|bytes| {
            serde_json::from_slice(&bytes).ok()
        })
    }

    /// Insert a cache entry into the database
    fn insert_entry(&self, cache_key: &str, entry: &CacheEntry) -> Result<()> {
        let value = serde_json::to_vec(entry)
            .context("Failed to serialize cache entry")?;
        self.db.insert(cache_key.as_bytes(), value)
            .context("Failed to insert cache entry")?;
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
        let (prefix, rest) = checksum.split_at(2.min(checksum.len()));
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

    /// Cache the outputs of a successful build
    pub fn cache_outputs(&self, cache_key: &str, input_checksum: &str, output_paths: &[PathBuf]) -> Result<()> {
        let mut outputs = Vec::new();

        for output_path in output_paths {
            if !output_path.exists() {
                continue;
            }

            let content = fs::read(output_path)?;
            let checksum = self.store_object(&content)?;
            let rel_path = Self::path_string(output_path);

            // Push object to remote cache if enabled
            if self.remote_push {
                self.try_push_object_to_remote(&checksum)?;
            }

            outputs.push(OutputEntry {
                path: rel_path,
                checksum,
            });
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

        Ok(())
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

    /// Save the index to disk (flush sled)
    pub fn save(&self) -> Result<()> {
        self.db.flush()
            .context("Failed to flush cache database")?;
        Ok(())
    }

    /// Convert path to string for storage. Paths are already relative.
    fn path_string(path: &Path) -> String {
        path.display().to_string()
    }

    /// Clear the entire cache
    pub fn clear(&mut self) -> Result<()> {
        // Drop the database before removing the directory
        // We need to reopen after clearing
        drop(std::mem::replace(&mut self.db, sled::Config::new().temporary(true).open().unwrap()));

        if self.rsb_dir.exists() {
            fs::remove_dir_all(&self.rsb_dir)
                .context("Failed to remove .rsb directory")?;
        }

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
        for result in self.db.iter() {
            let (_, value) = result.context("Failed to read cache entry during trim")?;
            if let Ok(entry) = serde_json::from_slice::<CacheEntry>(&value) {
                for output in &entry.outputs {
                    referenced.insert(output.checksum.clone());
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
        let mut stale_keys: Vec<Vec<u8>> = Vec::new();

        for result in self.db.iter() {
            if let Ok((key, _)) = result {
                if let Ok(key_str) = std::str::from_utf8(&key) {
                    if !valid_keys.contains(key_str) {
                        stale_keys.push(key.to_vec());
                    }
                }
            }
        }

        for key in stale_keys {
            if self.db.remove(&key).is_ok() {
                count += 1;
            }
        }

        count
    }

    /// List all cache entries with their status
    pub fn list(&self) -> Vec<CacheListEntry> {
        let mut entries: Vec<CacheListEntry> = self.db.iter()
            .filter_map(|result| {
                let (key, value) = result.ok()?;
                let key_str = std::str::from_utf8(&key).ok()?.to_string();
                let entry: CacheEntry = serde_json::from_slice(&value).ok()?;
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
