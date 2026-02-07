//! Dependency cache for storing source file dependencies.
//!
//! Uses a redb key/value store to cache dependency information discovered
//! from source files. This avoids re-scanning files that haven't changed.
//!
//! Cache key: source file path
//! Cache value: (source_checksum, dependencies)
//!
//! The cache is invalidated when the source file's checksum changes.

use anyhow::{Context, Result};
use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

const RSB_DIR: &str = ".rsb";
const DEPS_DB_FILE: &str = "deps.redb";

const DEPS_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("deps");

/// Cached dependency entry
#[derive(Debug, Serialize, Deserialize)]
struct DepsEntry {
    /// Checksum of the source file when dependencies were scanned
    source_checksum: String,
    /// List of dependency paths (relative to project root)
    dependencies: Vec<String>,
    /// Name of the analyzer that created this entry (e.g., "cpp", "python")
    #[serde(default)]
    analyzer: String,
}

/// Statistics about dependency cache usage
#[derive(Debug, Default, Clone)]
pub struct DepsCacheStats {
    /// Number of cache hits
    pub hits: usize,
    /// Number of cache misses
    pub misses: usize,
}

/// Dependency cache using redb key/value store
pub struct DepsCache {
    db: Database,
    stats: DepsCacheStats,
}

impl DepsCache {
    /// Open or create the dependency cache
    pub fn open() -> Result<Self> {
        let rsb_dir = PathBuf::from(RSB_DIR);
        let db_path = rsb_dir.join(DEPS_DB_FILE);

        // Ensure .rsb directory exists
        fs::create_dir_all(&rsb_dir)
            .context("Failed to create .rsb directory")?;

        // Open redb database, with delete-and-retry on corruption
        let db = match Database::create(&db_path) {
            Ok(db) => db,
            Err(_) => {
                eprintln!("Warning: Dependency cache corrupted, recreating");
                let _ = fs::remove_file(&db_path);
                Database::create(&db_path)
                    .context("Failed to create dependency cache database")?
            }
        };

        Ok(Self { db, stats: DepsCacheStats::default() })
    }

    /// Get cached dependencies for a source file if the cache is valid.
    /// Returns None if the file has changed or isn't cached.
    /// Updates internal statistics (hits/misses).
    pub fn get(&mut self, source: &Path) -> Option<Vec<PathBuf>> {
        let key = path_to_key(source);

        // Get cached entry
        let read_txn = self.db.begin_read().ok()?;
        let table = read_txn.open_table(DEPS_TABLE).ok()?;
        let data = match table.get(key.as_str()).ok()? {
            Some(d) => d,
            None => {
                self.stats.misses += 1;
                return None;
            }
        };
        let entry: DepsEntry = match serde_json::from_slice(data.value()) {
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

    /// Store dependencies for a source file with analyzer tag
    pub fn set(&self, source: &Path, dependencies: &[PathBuf], analyzer: &str) -> Result<()> {
        let key = path_to_key(source);
        let source_checksum = file_checksum(source)?;

        let entry = DepsEntry {
            source_checksum,
            dependencies: dependencies.iter()
                .map(|p| p.display().to_string())
                .collect(),
            analyzer: analyzer.to_string(),
        };

        let data = serde_json::to_vec(&entry)
            .context("Failed to serialize dependency entry")?;

        let write_txn = self.db.begin_write()
            .context("Failed to begin write transaction")?;
        {
            let mut table = write_txn.open_table(DEPS_TABLE)
                .context("Failed to open deps table")?;
            table.insert(key.as_str(), data.as_slice())
                .context("Failed to write to dependency cache")?;
        }
        write_txn.commit()
            .context("Failed to commit dependency cache write")?;

        Ok(())
    }

    /// Flush the cache to disk (no-op for redb — commits are per-transaction)
    pub fn flush(&self) -> Result<()> {
        Ok(())
    }

    /// Get cache statistics (hits and misses)
    pub fn stats(&self) -> &DepsCacheStats {
        &self.stats
    }

    /// Clear all cached dependencies
    #[allow(dead_code)]
    pub fn clear(&self) -> Result<()> {
        let write_txn = self.db.begin_write()
            .context("Failed to begin write transaction")?;
        let _ = write_txn.delete_table(DEPS_TABLE);
        write_txn.commit()
            .context("Failed to commit dependency cache clear")?;
        Ok(())
    }

    /// Get raw cached dependencies for a source file without validation.
    /// Returns None if the file isn't in the cache.
    /// Returns (dependencies, analyzer_name).
    pub fn get_raw(&self, source: &Path) -> Option<(Vec<PathBuf>, String)> {
        let key = path_to_key(source);
        let read_txn = self.db.begin_read().ok()?;
        let table = read_txn.open_table(DEPS_TABLE).ok()?;
        let data = table.get(key.as_str()).ok()??;
        let entry: DepsEntry = serde_json::from_slice(data.value()).ok()?;
        Some((
            entry.dependencies.iter().map(PathBuf::from).collect(),
            entry.analyzer,
        ))
    }

    /// List all cached source files and their dependencies.
    /// Returns tuples of (source_path, dependencies, analyzer_name).
    pub fn list_all(&self) -> Vec<(PathBuf, Vec<PathBuf>, String)> {
        let read_txn = match self.db.begin_read() {
            Ok(t) => t,
            Err(_) => return Vec::new(),
        };
        let table = match read_txn.open_table(DEPS_TABLE) {
            Ok(t) => t,
            Err(_) => return Vec::new(),
        };
        let iter = match table.iter() {
            Ok(i) => i,
            Err(_) => return Vec::new(),
        };
        iter.filter_map(|item| {
            let (key, value) = item.ok()?;
            let source = PathBuf::from(key.value().to_string());
            let entry: DepsEntry = serde_json::from_slice(value.value()).ok()?;
            let deps: Vec<PathBuf> = entry.dependencies.iter().map(PathBuf::from).collect();
            Some((source, deps, entry.analyzer))
        })
        .collect()
    }

    /// Get statistics about cached dependencies by analyzer.
    /// Returns a map of analyzer_name -> (file_count, total_dep_count).
    pub fn stats_by_analyzer(&self) -> std::collections::HashMap<String, (usize, usize)> {
        let mut stats: std::collections::HashMap<String, (usize, usize)> = std::collections::HashMap::new();
        let read_txn = match self.db.begin_read() {
            Ok(t) => t,
            Err(_) => return stats,
        };
        let table = match read_txn.open_table(DEPS_TABLE) {
            Ok(t) => t,
            Err(_) => return stats,
        };
        let iter = match table.iter() {
            Ok(i) => i,
            Err(_) => return stats,
        };
        for item in iter {
            if let Ok((_, value)) = item {
                if let Ok(entry) = serde_json::from_slice::<DepsEntry>(value.value()) {
                    let analyzer = if entry.analyzer.is_empty() { "unknown".to_string() } else { entry.analyzer };
                    let (files, deps) = stats.entry(analyzer).or_insert((0, 0));
                    *files += 1;
                    *deps += entry.dependencies.len();
                }
            }
        }
        stats
    }

    /// List cached source files and their dependencies filtered by analyzer names.
    /// Returns tuples of (source_path, dependencies, analyzer_name).
    pub fn list_by_analyzers(&self, analyzers: &[String]) -> Vec<(PathBuf, Vec<PathBuf>, String)> {
        let read_txn = match self.db.begin_read() {
            Ok(t) => t,
            Err(_) => return Vec::new(),
        };
        let table = match read_txn.open_table(DEPS_TABLE) {
            Ok(t) => t,
            Err(_) => return Vec::new(),
        };
        let iter = match table.iter() {
            Ok(i) => i,
            Err(_) => return Vec::new(),
        };
        iter.filter_map(|item| {
            let (key, value) = item.ok()?;
            let source = PathBuf::from(key.value().to_string());
            let entry: DepsEntry = serde_json::from_slice(value.value()).ok()?;
            if !analyzers.contains(&entry.analyzer) {
                return None;
            }
            let deps: Vec<PathBuf> = entry.dependencies.iter().map(PathBuf::from).collect();
            Some((source, deps, entry.analyzer))
        })
        .collect()
    }

    /// Remove all cached entries created by a specific analyzer.
    /// Returns the number of entries removed.
    pub fn remove_by_analyzer(&self, analyzer: &str) -> Result<usize> {
        // First, collect keys to remove by reading
        let keys_to_remove: Vec<String> = {
            let read_txn = self.db.begin_read()
                .context("Failed to begin read transaction")?;
            let table = match read_txn.open_table(DEPS_TABLE) {
                Ok(t) => t,
                Err(_) => return Ok(0),
            };
            let iter = table.iter()
                .context("Failed to iterate dependency cache")?;
            iter.filter_map(|item| {
                let (key, value) = item.ok()?;
                let entry: DepsEntry = serde_json::from_slice(value.value()).ok()?;
                if entry.analyzer == analyzer {
                    Some(key.value().to_string())
                } else {
                    None
                }
            })
            .collect()
        };

        let mut removed = 0;
        if !keys_to_remove.is_empty() {
            let write_txn = self.db.begin_write()
                .context("Failed to begin write transaction")?;
            {
                let mut table = write_txn.open_table(DEPS_TABLE)
                    .context("Failed to open deps table")?;
                for key in &keys_to_remove {
                    if table.remove(key.as_str()).is_ok() {
                        removed += 1;
                    }
                }
            }
            write_txn.commit()
                .context("Failed to commit dependency cache removal")?;
        }

        Ok(removed)
    }
}

/// Convert a path to a cache key
fn path_to_key(path: &Path) -> String {
    path.display().to_string()
}

/// Compute SHA-256 checksum of a file
fn file_checksum(path: &Path) -> Result<String> {
    let content = fs::read(path)
        .context(format!("Failed to read file for checksum: {}", path.display()))?;
    let hash = Sha256::digest(&content);
    Ok(hex::encode(hash))
}
