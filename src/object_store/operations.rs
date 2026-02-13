use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;

use super::{CacheEntry, ObjectStore, OutputEntry};

impl ObjectStore {
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
                    // Ensure object is available locally (fetch from remote if needed)
                    if !self.has_object(&out.checksum)
                        && (!self.remote_pull || !self.try_fetch_object_from_remote(&out.checksum)?)
                    {
                        return Ok(false);
                    }
                    if let Some(parent) = output_path.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    self.restore_file(&out.checksum, output_path)?;
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

        let (prefix, rest) = checksum.split_at(super::CHECKSUM_PREFIX_LEN.min(checksum.len()));
        let remote_key = format!("objects/{}/{}", prefix, rest);
        remote.download(&remote_key, &object_path)
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

        let (prefix, rest) = checksum.split_at(super::CHECKSUM_PREFIX_LEN.min(checksum.len()));
        let remote_key = format!("objects/{}/{}", prefix, rest);

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
}
