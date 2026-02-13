use anyhow::{Context, Result};
use redb::ReadableDatabase;
use std::path::PathBuf;
use std::time::SystemTime;

use super::{MtimeEntry, ObjectStore, MTIME_TABLE};

impl ObjectStore {
    /// Get the checksum for a file, using mtime to avoid re-reading unchanged files.
    /// If the file's mtime matches the cached entry, returns the cached checksum.
    /// Otherwise reads the file, computes SHA-256, and caches the result.
    fn fast_checksum(&self, file_path: &std::path::Path) -> Result<(String, Option<(String, MtimeEntry)>)> {
        let metadata = std::fs::metadata(file_path)
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

        let mut checksums = Vec::with_capacity(inputs.len());
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
        let mut checksums = Vec::with_capacity(inputs.len());
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
