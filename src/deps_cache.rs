//! Dependency cache for storing source file dependencies.
//!
//! Uses a sled key/value store to cache dependency information discovered
//! from source files. This avoids re-scanning files that haven't changed.
//!
//! Cache key: source file path
//! Cache value: (source_checksum, dependencies)
//!
//! The cache is invalidated when the source file's checksum changes.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

const RSB_DIR: &str = ".rsb";
const DEPS_DB_DIR: &str = "deps";

/// Cached dependency entry
#[derive(Debug, Serialize, Deserialize)]
struct DepsEntry {
    /// Checksum of the source file when dependencies were scanned
    source_checksum: String,
    /// List of dependency paths (relative to project root)
    dependencies: Vec<String>,
}

/// Statistics about dependency cache usage
#[derive(Debug, Default, Clone)]
pub struct DepsCacheStats {
    /// Number of cache hits
    pub hits: usize,
    /// Number of cache misses (recalculated)
    pub misses: usize,
}

/// Dependency cache using sled key/value store
pub struct DepsCache {
    db: sled::Db,
    stats: DepsCacheStats,
}

impl DepsCache {
    /// Open or create the dependency cache
    pub fn open() -> Result<Self> {
        let rsb_dir = PathBuf::from(RSB_DIR);
        let db_path = rsb_dir.join(DEPS_DB_DIR);

        // Ensure .rsb directory exists
        fs::create_dir_all(&rsb_dir)
            .context("Failed to create .rsb directory")?;

        let db = sled::open(&db_path)
            .context("Failed to open dependency cache database")?;

        Ok(Self { db, stats: DepsCacheStats::default() })
    }

    /// Get cached dependencies for a source file if the cache is valid.
    /// Returns None if the file has changed or isn't cached.
    /// Updates internal statistics (hits/misses).
    pub fn get(&mut self, source: &Path) -> Option<Vec<PathBuf>> {
        let key = path_to_key(source);

        // Get cached entry
        let data = match self.db.get(&key).ok()? {
            Some(d) => d,
            None => {
                self.stats.misses += 1;
                return None;
            }
        };
        let entry: DepsEntry = match serde_json::from_slice(&data) {
            Ok(e) => e,
            Err(_) => {
                self.stats.misses += 1;
                return None;
            }
        };

        // Verify source file hasn't changed
        let current_checksum = match file_checksum(source) {
            Ok(c) => c,
            Err(_) => {
                self.stats.misses += 1;
                return None;
            }
        };
        if entry.source_checksum != current_checksum {
            self.stats.misses += 1;
            return None;
        }

        // Verify all dependencies still exist
        let deps: Vec<PathBuf> = entry.dependencies.iter()
            .map(PathBuf::from)
            .collect();

        for dep in &deps {
            if !dep.exists() {
                self.stats.misses += 1;
                return None;
            }
        }

        self.stats.hits += 1;
        Some(deps)
    }

    /// Store dependencies for a source file
    pub fn set(&self, source: &Path, dependencies: &[PathBuf]) -> Result<()> {
        let key = path_to_key(source);
        let source_checksum = file_checksum(source)?;

        let entry = DepsEntry {
            source_checksum,
            dependencies: dependencies.iter()
                .map(|p| p.display().to_string())
                .collect(),
        };

        let data = serde_json::to_vec(&entry)
            .context("Failed to serialize dependency entry")?;

        self.db.insert(&key, data)
            .context("Failed to write to dependency cache")?;

        Ok(())
    }

    /// Flush the cache to disk
    pub fn flush(&self) -> Result<()> {
        self.db.flush()
            .context("Failed to flush dependency cache")?;
        Ok(())
    }

    /// Get cache statistics (hits and misses)
    pub fn stats(&self) -> &DepsCacheStats {
        &self.stats
    }

    /// Clear all cached dependencies
    #[allow(dead_code)]
    pub fn clear(&self) -> Result<()> {
        self.db.clear()
            .context("Failed to clear dependency cache")?;
        Ok(())
    }

    /// Get raw cached dependencies for a source file without validation.
    /// Returns None if the file isn't in the cache.
    pub fn get_raw(&self, source: &Path) -> Option<Vec<PathBuf>> {
        let key = path_to_key(source);
        let data = self.db.get(&key).ok()??;
        let entry: DepsEntry = serde_json::from_slice(&data).ok()?;
        Some(entry.dependencies.iter().map(PathBuf::from).collect())
    }

    /// List all cached source files and their dependencies.
    /// Returns pairs of (source_path, dependencies).
    pub fn list_all(&self) -> Vec<(PathBuf, Vec<PathBuf>)> {
        self.db.iter()
            .filter_map(|item| {
                let (key, value) = item.ok()?;
                let source = PathBuf::from(String::from_utf8(key.to_vec()).ok()?);
                let entry: DepsEntry = serde_json::from_slice(&value).ok()?;
                let deps: Vec<PathBuf> = entry.dependencies.iter().map(PathBuf::from).collect();
                Some((source, deps))
            })
            .collect()
    }
}

/// Convert a path to a cache key
fn path_to_key(path: &Path) -> Vec<u8> {
    path.display().to_string().into_bytes()
}

/// Compute SHA-256 checksum of a file
fn file_checksum(path: &Path) -> Result<String> {
    let content = fs::read(path)
        .context(format!("Failed to read file for checksum: {}", path.display()))?;
    let hash = Sha256::digest(&content);
    Ok(hex::encode(hash))
}
