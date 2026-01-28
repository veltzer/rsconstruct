use anyhow::{Context, Result};
use sha2::{Sha256, Digest};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::config::RestoreMethod;

const RSB_DIR: &str = ".rsb";
const OBJECTS_DIR: &str = "objects";
const INDEX_FILE: &str = "index.json";

/// Object store for caching build outputs
/// Uses git-like object storage: .rsb/objects/[2 chars]/[rest of hash]
#[derive(Debug)]
pub struct ObjectStore {
    /// Root directory of the project
    project_root: PathBuf,
    /// Path to .rsb directory
    rsb_dir: PathBuf,
    /// Path to objects directory
    objects_dir: PathBuf,
    /// Index mapping cache keys to stored checksums
    index: CacheIndex,
    /// Method to restore files from cache
    restore_method: RestoreMethod,
}

impl Default for ObjectStore {
    fn default() -> Self {
        let project_root = PathBuf::from(".");
        let rsb_dir = project_root.join(RSB_DIR);
        let objects_dir = rsb_dir.join(OBJECTS_DIR);
        Self {
            project_root,
            rsb_dir,
            objects_dir,
            index: CacheIndex::default(),
            restore_method: RestoreMethod::default(),
        }
    }
}

/// Index file that maps product cache keys to their stored output checksums
#[derive(Debug, Serialize, Deserialize, Default)]
struct CacheIndex {
    /// Map from cache key (e.g., "template:/path/to/input") to output info
    entries: HashMap<String, CacheEntry>,
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

impl ObjectStore {
    pub fn new(project_root: PathBuf, restore_method: RestoreMethod) -> Result<Self> {
        let rsb_dir = project_root.join(RSB_DIR);
        let objects_dir = rsb_dir.join(OBJECTS_DIR);

        // Load existing index or create new one
        let index_path = rsb_dir.join(INDEX_FILE);
        let index = if index_path.exists() {
            let content = fs::read_to_string(&index_path)
                .context("Failed to read cache index")?;
            serde_json::from_str(&content)
                .context("Failed to parse cache index")?
        } else {
            CacheIndex::default()
        };

        Ok(Self {
            project_root,
            rsb_dir,
            objects_dir,
            index,
            restore_method,
        })
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
                // Try hard link first, fall back to copy if it fails
                if fs::hard_link(&object_path, output_path).is_err() {
                    // Hard link failed (e.g., cross-filesystem), fall back to copy
                    fs::copy(&object_path, output_path)
                        .with_context(|| format!("Failed to copy from cache: {}", checksum))?;
                }
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
        let entry = match self.index.entries.get(cache_key) {
            Some(e) => e,
            None => return true,
        };

        // Check if input checksum matches
        if entry.input_checksum != input_checksum {
            return true;
        }

        // Check if all outputs exist at their original paths
        for output_path in output_paths {
            if !output_path.exists() {
                // Output missing - check if we can restore from cache
                let rel_path = self.relative_path(output_path);
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
        let entry = match self.index.entries.get(cache_key) {
            Some(e) if e.input_checksum == input_checksum => e,
            _ => return false,
        };

        for output_path in output_paths {
            if output_path.exists() {
                continue;
            }

            let rel_path = self.relative_path(output_path);
            let cached_output = entry.outputs.iter()
                .find(|o| o.path == rel_path);

            match cached_output {
                Some(out) if self.has_object(&out.checksum) => {}
                _ => return false,
            }
        }

        true
    }

    /// Try to restore outputs from cache
    /// Returns true if all outputs were restored
    pub fn restore_from_cache(&self, cache_key: &str, input_checksum: &str, output_paths: &[PathBuf]) -> Result<bool> {
        // Check if we have a cache entry with matching input checksum
        let entry = match self.index.entries.get(cache_key) {
            Some(e) if e.input_checksum == input_checksum => e,
            _ => return Ok(false),
        };

        // Try to restore each missing output
        for output_path in output_paths {
            if output_path.exists() {
                continue;
            }

            let rel_path = self.relative_path(output_path);
            let cached_output = entry.outputs.iter()
                .find(|o| o.path == rel_path);

            match cached_output {
                Some(out) if self.has_object(&out.checksum) => {
                    // Restore from cache
                    if let Some(parent) = output_path.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    self.restore_file(&out.checksum, output_path)?;
                }
                _ => return Ok(false),
            }
        }

        Ok(true)
    }

    /// Cache the outputs of a successful build
    pub fn cache_outputs(&mut self, cache_key: &str, input_checksum: &str, output_paths: &[PathBuf]) -> Result<()> {
        let mut outputs = Vec::new();

        for output_path in output_paths {
            if !output_path.exists() {
                continue;
            }

            let content = fs::read(output_path)?;
            let checksum = self.store_object(&content)?;
            let rel_path = self.relative_path(output_path);

            outputs.push(OutputEntry {
                path: rel_path,
                checksum,
            });
        }

        self.index.entries.insert(cache_key.to_string(), CacheEntry {
            input_checksum: input_checksum.to_string(),
            outputs,
        });

        Ok(())
    }

    /// Save the index to disk
    pub fn save(&self) -> Result<()> {
        fs::create_dir_all(&self.rsb_dir)?;
        let index_path = self.rsb_dir.join(INDEX_FILE);
        let content = serde_json::to_string_pretty(&self.index)?;
        fs::write(&index_path, content)?;
        Ok(())
    }

    /// Get relative path from project root
    fn relative_path(&self, path: &Path) -> String {
        path.strip_prefix(&self.project_root)
            .unwrap_or(path)
            .display()
            .to_string()
    }

    /// Clear the entire cache
    pub fn clear(&mut self) -> Result<()> {
        self.index.entries.clear();

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

        for entry in WalkDir::new(&self.objects_dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
        {
            if let Ok(metadata) = entry.metadata() {
                total_bytes += metadata.len();
                object_count += 1;
            }
        }

        Ok((total_bytes, object_count))
    }

    /// Trim cache by removing objects not referenced in the index
    pub fn trim(&mut self) -> Result<(u64, usize)> {
        let mut removed_bytes = 0u64;
        let mut removed_count = 0usize;

        if !self.objects_dir.exists() {
            return Ok((0, 0));
        }

        // Collect all referenced checksums
        let mut referenced: std::collections::HashSet<String> = std::collections::HashSet::new();
        for entry in self.index.entries.values() {
            for output in &entry.outputs {
                referenced.insert(output.checksum.clone());
            }
        }

        // Find and remove unreferenced objects
        let mut to_remove = Vec::new();
        for entry in WalkDir::new(&self.objects_dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
        {
            // Reconstruct checksum from path
            let path = entry.path();
            if let (Some(prefix), Some(rest)) = (
                path.parent().and_then(|p| p.file_name()).and_then(|n| n.to_str()),
                path.file_name().and_then(|n| n.to_str())
            ) {
                let checksum = format!("{}{}", prefix, rest);
                if !referenced.contains(&checksum) {
                    if let Ok(metadata) = entry.metadata() {
                        removed_bytes += metadata.len();
                        removed_count += 1;
                    }
                    to_remove.push(path.to_path_buf());
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

    /// Get the combined input checksum for a list of input files
    pub fn combined_input_checksum(inputs: &[PathBuf]) -> Result<String> {
        let mut checksums = Vec::new();
        for input in inputs {
            if input.exists() {
                checksums.push(Self::calculate_checksum(input)?);
            }
        }
        Ok(checksums.join(":"))
    }
}
