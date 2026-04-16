use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use super::{ObjectStore, CHECKSUM_PREFIX_LEN};
use crate::checksum;
use crate::config::RestoreMethod;

impl ObjectStore {
    /// Calculate SHA-256 checksum of a file
    pub fn calculate_checksum(file_path: &Path) -> Result<String> {
        checksum::file_checksum(file_path)
    }

    /// Calculate SHA-256 checksum of bytes
    pub fn calculate_checksum_bytes(data: &[u8]) -> String {
        checksum::bytes_checksum(data)
    }

    /// Get object path for a checksum (e.g., .rsconstruct/objects/ab/cdef123...)
    pub(super) fn object_path(&self, checksum: &str) -> PathBuf {
        let (prefix, rest) = checksum.split_at(CHECKSUM_PREFIX_LEN.min(checksum.len()));
        self.objects_dir.join(prefix).join(rest)
    }

    /// Store content in object store, returns checksum.
    /// The checksum is always computed on the **original** (uncompressed) content
    /// so cache keys remain stable regardless of compression setting.
    /// Objects are made read-only to prevent accidental modification via hardlinks.
    pub(super) fn store_object(&self, content: &[u8]) -> Result<String> {
        let checksum = Self::calculate_checksum_bytes(content);
        let object_path = self.object_path(&checksum);

        if !object_path.exists() {
            if let Some(parent) = object_path.parent() {
                fs::create_dir_all(parent)
                    .context("Failed to create object directory")?;
            }
            let blob = if self.compression {
                zstd::encode_all(content, 0)
                    .context("Failed to zstd-compress object")?
            } else {
                content.to_vec()
            };
            fs::write(&object_path, &blob)
                .context("Failed to write object")?;
            let mut perms = fs::metadata(&object_path)
                .context("Failed to read object metadata")?
                .permissions();
            perms.set_readonly(true);
            fs::set_permissions(&object_path, perms)
                .context("Failed to set object read-only")?;
        }

        Ok(checksum)
    }

    /// Check if an object exists in the store
    pub(super) fn has_object(&self, checksum: &str) -> bool {
        self.object_path(checksum).exists()
    }

    /// Restore a file from the object store using configured method.
    pub(super) fn restore_file(&self, checksum: &str, output_path: &Path) -> Result<()> {
        let object_path = self.object_path(checksum);

        if self.compression {
            let content = self.read_object(checksum)
                .with_context(|| format!("Failed to read cached object: {}", checksum))?;
            fs::write(output_path, &content)
                .with_context(|| format!("Failed to write decompressed output: {}", output_path.display()))?;
            crate::platform::set_permissions_mode(output_path, 0o644)
                .context("Failed to make restored file writable")?;
            return Ok(());
        }

        match self.restore_method {
            RestoreMethod::Hardlink => {
                fs::hard_link(&object_path, output_path)
                    .with_context(|| format!("Failed to hard link from cache: {}. If on a cross-filesystem setup, set restore_method = \"copy\" in rsconstruct.toml.", checksum))?;
            }
            RestoreMethod::Copy => {
                fs::copy(&object_path, output_path)
                    .with_context(|| format!("Failed to copy from cache: {}", checksum))?;
                crate::platform::set_permissions_mode(output_path, 0o644)
                    .context("Failed to make restored file writable")?;
            }
            RestoreMethod::Auto => unreachable!("Auto should be resolved before use"),
        }

        Ok(())
    }

    /// Read and optionally decompress an object from the store.
    pub(crate) fn read_object(&self, checksum: &str) -> Result<Vec<u8>> {
        let object_path = self.object_path(checksum);
        let raw = fs::read(&object_path)
            .with_context(|| format!("Failed to read object: {}", checksum))?;
        if self.compression {
            zstd::decode_all(raw.as_slice())
                .with_context(|| format!("Failed to decompress object: {}", checksum))
        } else {
            Ok(raw)
        }
    }

    /// Convert path to string for storage. Paths are already relative.
    pub(super) fn path_string(path: &Path) -> String {
        path.display().to_string()
    }
}
