use anyhow::{Context, Result};
use redb::{Database, ReadableDatabase, ReadableTable};
use std::collections::BTreeMap;
use std::fs;

use super::{
    walk_files, CacheEntry, CacheListEntry, CacheListOutput, ObjectStore,
    ProcessorCacheStats, CACHE_TABLE,
};

impl ObjectStore {
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
}
