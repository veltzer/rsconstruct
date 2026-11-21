use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::fs;

use super::{
    CacheDescriptor, CacheListEntry, CacheListOutput, ObjectStore, ProcessorCacheStats, walk_files,
};

/// Parse one stored descriptor. Every management command goes through this,
/// so a damaged descriptor is the same named error in `list`, `stats` and
/// `trim` instead of an entry two of them leave out.
fn parse_descriptor(key: &str, data: &[u8]) -> Result<CacheDescriptor> {
    serde_json::from_slice(data).with_context(|| {
        format!(
            "Failed to parse cache descriptor {key} (run `rsconstruct cache clear` to start the \
             cache over)"
        )
    })
}

/// What the cache holds: its total size, and how many objects (the stored
/// content of outputs) and entries (one descriptor per cached product) make
/// it up.
pub struct CacheSize {
    pub bytes: u64,
    pub objects: usize,
    pub entries: usize,
}

impl ObjectStore {
    /// Get the cache's size: the objects on disk plus the stored
    /// descriptors. Objects and entries are counted apart: lumped together
    /// as "objects" the figure matched neither `cache trim`, which calls
    /// only the stored content objects, nor `cache stats`, which counts
    /// entries.
    pub fn size(&self) -> Result<CacheSize> {
        let mut size = CacheSize {
            bytes: 0,
            objects: 0,
            entries: 0,
        };
        if self.objects_dir.exists() {
            for path in walk_files(&self.objects_dir)? {
                let metadata = fs::metadata(&path)
                    .with_context(|| format!("Failed to stat cache file {}", path.display()))?;
                size.bytes += metadata.len();
                size.objects += 1;
            }
        }
        for (key, data) in self.descriptor_rows()? {
            size.bytes += (key.len() + data.len()) as u64;
            size.entries += 1;
        }
        Ok(size)
    }

    /// Trim cache by removing blob objects not referenced by any descriptor.
    pub fn trim(&self) -> Result<(u64, usize)> {
        let mut removed_bytes = 0u64;
        let mut removed_count = 0usize;

        if !self.objects_dir.exists() {
            return Ok((0, 0));
        }

        // Collect all referenced blob checksums from descriptors. An
        // unreadable or unparsable descriptor must abort the trim: skipping it
        // would garbage-collect every blob it references as "unreferenced".
        let mut referenced: std::collections::HashSet<String> = std::collections::HashSet::new();
        for (key, data) in self.descriptor_rows()? {
            match parse_descriptor(&key, &data)? {
                CacheDescriptor::Marker => {}
                CacheDescriptor::Blob { checksum, .. } => {
                    referenced.insert(checksum);
                }
                CacheDescriptor::Tree { entries } => {
                    for entry in entries {
                        if entry.link.is_none() {
                            referenced.insert(entry.checksum);
                        }
                    }
                }
            }
        }

        // Find and remove unreferenced blob objects
        let mut to_remove = Vec::new();
        for path in walk_files(&self.objects_dir)? {
            if let (Some(prefix), Some(rest)) = (
                path.parent()
                    .and_then(|p| p.file_name())
                    .and_then(|n| n.to_str()),
                path.file_name().and_then(|n| n.to_str()),
            ) {
                // Compressed objects carry a .zst suffix; strip it to recover
                // the checksum. Stray temp files never match a referenced
                // checksum and are collected as garbage here.
                let rest = rest.strip_suffix(".zst").unwrap_or(rest);
                let checksum = format!("{prefix}{rest}");
                if !referenced.contains(&checksum) {
                    if let Ok(metadata) = fs::metadata(&path) {
                        removed_bytes += metadata.len();
                        removed_count += 1;
                    }
                    to_remove.push(path);
                }
            }
        }

        for path in to_remove {
            // Make writable before removing (objects are stored read-only so a
            // restored hardlink can't corrupt the cache). The file is unlinked
            // on the next line, so the widened mode never outlives this loop
            // iteration — which is what the lint is warning about.
            #[allow(clippy::permissions_set_readonly_false)]
            if let Ok(mut perms) = fs::metadata(&path).map(|m| m.permissions()) {
                perms.set_readonly(false);
                fs::set_permissions(&path, perms).with_context(|| {
                    format!("Failed to make cache object writable: {}", path.display())
                })?;
            }
            fs::remove_file(&path)
                .with_context(|| format!("Failed to remove cache object: {}", path.display()))?;
            if let Some(parent) = path.parent() {
                // Best-effort: remove empty parent dir (fails silently if not empty)
                let _ = fs::remove_dir(parent);
            }
        }

        Ok((removed_bytes, removed_count))
    }

    /// Remove the descriptor stored under `descriptor_key`, if there is one,
    /// so the product it describes builds again instead of restoring. The
    /// blobs it pointed at stay for `trim`. Returns whether one was removed.
    pub fn remove_descriptor(&self, descriptor_key: &str) -> Result<bool> {
        let mut removed = false;
        self.write_descriptors(|table| {
            removed = table
                .remove(descriptor_key)
                .with_context(|| format!("Failed to remove cache descriptor {descriptor_key}"))?
                .is_some();
            Ok(())
        })?;
        Ok(removed)
    }

    /// Remove stale descriptor entries whose cache keys are not in the valid set.
    /// Returns the number of entries removed.
    pub fn remove_stale(
        &self,
        valid_descriptor_keys: &std::collections::HashSet<String>,
    ) -> Result<usize> {
        let stale: Vec<String> = self
            .descriptor_rows()?
            .into_iter()
            .map(|(key, _)| key)
            .filter(|key| !valid_descriptor_keys.contains(key))
            .collect();
        if stale.is_empty() {
            return Ok(0);
        }
        self.write_descriptors(|table| {
            for key in &stale {
                table
                    .remove(key.as_str())
                    .with_context(|| format!("Failed to remove stale cache descriptor {key}"))?;
            }
            Ok(())
        })?;
        Ok(stale.len())
    }

    /// List all cache descriptors
    pub fn list(&self) -> Result<Vec<CacheListEntry>> {
        let mut entries: Vec<CacheListEntry> = Vec::new();
        for (cache_key, data) in self.descriptor_rows()? {
            let desc = parse_descriptor(&cache_key, &data)?;
            let outputs = match desc {
                CacheDescriptor::Marker => Vec::new(),
                CacheDescriptor::Blob { ref checksum, .. } => {
                    vec![CacheListOutput {
                        path: "(blob)".to_string(),
                        exists: self.has_object(checksum),
                    }]
                }
                CacheDescriptor::Tree { entries } => entries
                    .iter()
                    .map(|e| CacheListOutput {
                        path: e.path.clone(),
                        // A symlink has no object to be missing.
                        exists: e.link.is_some() || self.has_object(&e.checksum),
                    })
                    .collect(),
            };

            entries.push(CacheListEntry { cache_key, outputs });
        }

        entries.sort_by(|a, b| a.cache_key.cmp(&b.cache_key));
        Ok(entries)
    }

    /// Get per-processor cache statistics.
    /// Extracts processor name by scanning descriptor keys.
    pub fn stats_by_processor(&self) -> Result<BTreeMap<String, ProcessorCacheStats>> {
        let mut stats: BTreeMap<String, ProcessorCacheStats> = BTreeMap::new();

        for (key, data) in self.descriptor_rows()? {
            let desc = parse_descriptor(&key, &data)?;

            // We can't extract processor name from a hashed descriptor key.
            // Use "all" as a single bucket for now.
            let processor = "all".to_string();
            let proc_stats = stats.entry(processor).or_default();
            proc_stats.entry_count += 1;

            match desc {
                CacheDescriptor::Marker => {}
                CacheDescriptor::Blob { ref checksum, .. } => {
                    proc_stats.output_count += 1;
                    proc_stats.output_bytes += self.object_size(checksum);
                }
                CacheDescriptor::Tree { ref entries } => {
                    proc_stats.output_count += entries.len();
                    for entry in entries {
                        if entry.link.is_none() {
                            proc_stats.output_bytes += self.object_size(&entry.checksum);
                        }
                    }
                }
            }
        }

        Ok(stats)
    }
}
