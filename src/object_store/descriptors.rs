use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use super::{CacheDescriptor, ObjectStore, TreeEntry, CHECKSUM_PREFIX_LEN, walk_files};

impl ObjectStore {
    pub(super) fn descriptor_path(&self, descriptor_key: &str) -> PathBuf {
        let (prefix, rest) = descriptor_key.split_at(CHECKSUM_PREFIX_LEN.min(descriptor_key.len()));
        self.descriptors_dir.join(prefix).join(rest)
    }

    /// Store a cache descriptor for a cache key.
    pub(super) fn store_descriptor(&self, cache_key: &str, descriptor: &CacheDescriptor) -> Result<()> {
        let path = self.descriptor_path(cache_key);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .context("Failed to create descriptor directory")?;
        }
        let data = serde_json::to_vec(descriptor)
            .context("Failed to serialize cache descriptor")?;
        if path.exists() {
            let mut perms = fs::metadata(&path)
                .with_context(|| format!("Failed to read metadata for descriptor: {}", path.display()))?.permissions();
            perms.set_readonly(false);
            fs::set_permissions(&path, perms)
                .with_context(|| format!("Failed to make descriptor writable: {}", path.display()))?;
        }
        fs::write(&path, &data)
            .with_context(|| format!("Failed to write cache descriptor: {}", path.display()))?;
        let mut perms = fs::metadata(&path)
            .with_context(|| format!("Failed to read metadata for descriptor: {}", path.display()))?.permissions();
        perms.set_readonly(true);
        fs::set_permissions(&path, perms)
            .with_context(|| format!("Failed to make descriptor read-only: {}", path.display()))?;
        Ok(())
    }

    /// Read a cache descriptor for a cache key. Returns None if not found.
    pub(super) fn get_descriptor(&self, cache_key: &str) -> Option<CacheDescriptor> {
        let path = self.descriptor_path(cache_key);
        let data = fs::read(&path).ok()?;
        serde_json::from_slice(&data).ok()
    }

    /// Return the list of file paths recorded in the product's last tree descriptor.
    pub fn previous_tree_paths(&self, cache_key: &str) -> Vec<PathBuf> {
        match self.get_descriptor(cache_key) {
            Some(CacheDescriptor::Tree { entries }) => {
                entries.into_iter().map(|e| PathBuf::from(e.path)).collect()
            }
            _ => Vec::new(),
        }
    }

    /// Store a marker descriptor (checker passed).
    pub fn store_marker(&self, cache_key: &str) -> Result<()> {
        self.store_descriptor(cache_key, &CacheDescriptor::Marker)
    }

    /// Store a blob descriptor (generator produced a single output).
    pub fn store_blob_descriptor(&self, ctx: &crate::build_context::BuildContext, cache_key: &str, output_path: &Path) -> Result<bool> {
        let content = fs::read(output_path)
            .with_context(|| format!("Failed to read output: {}", output_path.display()))?;
        let checksum = self.store_object(&content)?;
        let mode = fs::metadata(output_path).ok()
            .and_then(|m| crate::platform::get_mode(&m));

        let changed = match self.get_descriptor(cache_key) {
            Some(CacheDescriptor::Blob { checksum: prev, .. }) => prev != checksum,
            _ => true,
        };

        if self.remote_push {
            self.try_push_object_to_remote(ctx, &checksum)?;
        }

        self.store_descriptor(cache_key, &CacheDescriptor::Blob {
            checksum,
            mode,
        })?;

        Ok(changed)
    }

    /// Store a tree descriptor (creator produced multiple outputs).
    pub fn store_tree_descriptor(
        &self,
        ctx: &crate::build_context::BuildContext,
        cache_key: &str,
        output_dirs: &[std::sync::Arc<PathBuf>],
        output_files: &[PathBuf],
        is_foreign: &dyn Fn(&Path) -> bool,
    ) -> Result<bool> {
        let prev = self.get_descriptor(cache_key);
        let mut entries = Vec::new();

        for dir in output_dirs {
            let dir: &Path = dir;
            anyhow::ensure!(dir.exists() && dir.is_dir(),
                "Expected output directory not produced: {}", dir.display());
            for file_path in walk_files(dir) {
                if is_foreign(&file_path) {
                    continue;
                }
                let content = fs::read(&file_path)
                    .with_context(|| format!("Failed to read: {}", file_path.display()))?;
                let checksum = self.store_object(&content)?;
                let mode = fs::metadata(&file_path).ok()
                    .and_then(|m| crate::platform::get_mode(&m));
                if self.remote_push {
                    self.try_push_object_to_remote(ctx, &checksum)?;
                }
                entries.push(TreeEntry {
                    path: file_path.display().to_string(),
                    checksum,
                    mode,
                });
            }
        }

        for file_path in output_files {
            anyhow::ensure!(file_path.exists(),
                "Expected output file not produced: {}", file_path.display());
            let content = fs::read(file_path)
                .with_context(|| format!("Failed to read: {}", file_path.display()))?;
            let checksum = self.store_object(&content)?;
            let mode = fs::metadata(file_path).ok()
                .and_then(|m| crate::platform::get_mode(&m));
            if self.remote_push {
                self.try_push_object_to_remote(ctx, &checksum)?;
            }
            entries.push(TreeEntry {
                path: Self::path_string(file_path),
                checksum,
                mode,
            });
        }

        let changed = match prev {
            Some(CacheDescriptor::Tree { entries: ref prev_entries }) => {
                entries.len() != prev_entries.len()
                    || entries.iter().zip(prev_entries.iter()).any(|(a, b)| a.checksum != b.checksum || a.path != b.path)
            }
            _ => true,
        };

        self.store_descriptor(cache_key, &CacheDescriptor::Tree { entries })?;
        Ok(changed)
    }
}
