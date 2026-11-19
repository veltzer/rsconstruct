use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use super::{
    CacheDescriptor, CacheListEntry, CacheListOutput, ObjectStore, ProcessorCacheStats, walk_files,
};

/// Whether `path` is a temp file a crashed descriptor write left behind
/// (`store_descriptor` writes `.tmp-<pid>-<n>`, then renames). It is not a
/// descriptor and may be truncated.
fn is_descriptor_temp(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with(".tmp-"))
}

/// Read and parse the descriptor file at `path`. Every management command
/// goes through this, so a damaged descriptor is the same named error in
/// `list`, `stats` and `trim` instead of an entry two of them leave out.
fn read_descriptor_file(path: &Path) -> Result<CacheDescriptor> {
    let data = fs::read(path)
        .with_context(|| format!("Failed to read cache descriptor: {}", path.display()))?;
    serde_json::from_slice(&data).with_context(|| {
        format!(
            "Failed to parse cache descriptor: {} (remove it to proceed)",
            path.display()
        )
    })
}

impl ObjectStore {
    /// Get cache size in bytes and number of objects (blobs + descriptors)
    pub fn size(&self) -> Result<(u64, usize)> {
        let mut total_bytes = 0u64;
        let mut object_count = 0usize;

        for dir in [&self.objects_dir, &self.descriptors_dir] {
            if !dir.exists() {
                continue;
            }
            for path in walk_files(dir)? {
                let metadata = fs::metadata(&path)
                    .with_context(|| format!("Failed to stat cache file {}", path.display()))?;
                total_bytes += metadata.len();
                object_count += 1;
            }
        }

        Ok((total_bytes, object_count))
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
        if self.descriptors_dir.exists() {
            for path in walk_files(&self.descriptors_dir)? {
                // Parsing a leftover temp file would abort every trim until
                // someone removed it by hand. It is garbage, so it is
                // collected here. redb's exclusive lock means no other
                // process is mid-write.
                if is_descriptor_temp(&path) {
                    fs::remove_file(&path).with_context(|| {
                        format!(
                            "Failed to remove leftover descriptor temp file: {}",
                            path.display()
                        )
                    })?;
                    continue;
                }
                match read_descriptor_file(&path)? {
                    CacheDescriptor::Marker => {}
                    CacheDescriptor::Blob { checksum, .. } => {
                        referenced.insert(checksum);
                    }
                    CacheDescriptor::Tree { entries } => {
                        for entry in entries {
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
        let path = self.descriptor_path(descriptor_key);
        if !path.is_file() {
            return Ok(false);
        }
        // Descriptors are read-only (see `store_descriptor`); make this one
        // writable for the unlink that follows, as `trim` does.
        #[allow(clippy::permissions_set_readonly_false)]
        if let Ok(mut perms) = fs::metadata(&path).map(|m| m.permissions()) {
            perms.set_readonly(false);
            fs::set_permissions(&path, perms).with_context(|| {
                format!("Failed to make descriptor writable: {}", path.display())
            })?;
        }
        fs::remove_file(&path)
            .with_context(|| format!("Failed to remove descriptor: {}", path.display()))?;
        if let Some(parent) = path.parent() {
            // Best-effort: remove empty parent dir (fails silently if not empty)
            let _ = fs::remove_dir(parent);
        }
        Ok(true)
    }

    /// Remove stale descriptor entries whose cache keys are not in the valid set.
    /// Returns the number of entries removed.
    pub fn remove_stale(
        &self,
        valid_descriptor_keys: &std::collections::HashSet<String>,
    ) -> Result<usize> {
        let mut count = 0;

        if !self.descriptors_dir.exists() {
            return Ok(0);
        }

        for path in walk_files(&self.descriptors_dir)? {
            // Reconstruct descriptor key from path
            if let (Some(prefix), Some(rest)) = (
                path.parent()
                    .and_then(|p| p.file_name())
                    .and_then(|n| n.to_str()),
                path.file_name().and_then(|n| n.to_str()),
            ) {
                let key = format!("{prefix}{rest}");
                if !valid_descriptor_keys.contains(&key) {
                    // Same as in `trim`: descriptors are read-only, and this
                    // one is unlinked immediately below.
                    #[allow(clippy::permissions_set_readonly_false)]
                    if let Ok(mut perms) = fs::metadata(&path).map(|m| m.permissions()) {
                        perms.set_readonly(false);
                        fs::set_permissions(&path, perms).with_context(|| {
                            format!(
                                "Failed to make stale descriptor writable: {}",
                                path.display()
                            )
                        })?;
                    }
                    fs::remove_file(&path).with_context(|| {
                        format!("Failed to remove stale descriptor: {}", path.display())
                    })?;
                    count += 1;
                    if let Some(parent) = path.parent() {
                        // Best-effort: remove empty parent dir (fails silently if not empty)
                        let _ = fs::remove_dir(parent);
                    }
                }
            }
        }

        Ok(count)
    }

    /// List all cache descriptors
    pub fn list(&self) -> Result<Vec<CacheListEntry>> {
        if !self.descriptors_dir.exists() {
            return Ok(Vec::new());
        }

        let mut entries: Vec<CacheListEntry> = Vec::new();
        for path in walk_files(&self.descriptors_dir)? {
            if is_descriptor_temp(&path) {
                continue;
            }
            let desc = read_descriptor_file(&path)?;

            // Reconstruct descriptor key from path
            let (Some(prefix), Some(rest)) = (
                path.parent()
                    .and_then(|p| p.file_name())
                    .and_then(|n| n.to_str()),
                path.file_name().and_then(|n| n.to_str()),
            ) else {
                anyhow::bail!(
                    "Cache descriptor path is not valid UTF-8: {}",
                    path.display()
                );
            };
            let cache_key = format!("{prefix}{rest}");

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
                        exists: self.has_object(&e.checksum),
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

        if !self.descriptors_dir.exists() {
            return Ok(stats);
        }

        for path in walk_files(&self.descriptors_dir)? {
            if is_descriptor_temp(&path) {
                continue;
            }
            let desc = read_descriptor_file(&path)?;

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
                        proc_stats.output_bytes += self.object_size(&entry.checksum);
                    }
                }
            }
        }

        Ok(stats)
    }
}
