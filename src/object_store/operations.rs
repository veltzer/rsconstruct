use anyhow::{Context, Result};
use std::fs;

use super::ObjectStore;

impl ObjectStore {
    /// Try to push an object to remote cache (ignores errors)
    #[allow(clippy::unnecessary_wraps)] // Result kept for API symmetry with try_fetch_*; future writers may legitimately fail.
    pub(super) fn try_push_object_to_remote(&self, ctx: &crate::build_context::BuildContext, checksum: &str) -> Result<()> {
        let Some(remote) = &self.remote else { return Ok(()) };

        let object_path = self.object_path(checksum);
        if !object_path.exists() {
            return Ok(());
        }

        let (prefix, rest) = checksum.split_at(super::CHECKSUM_PREFIX_LEN.min(checksum.len()));
        let remote_key = format!("objects/{prefix}/{rest}");

        // Check if already exists remotely (avoid redundant uploads)
        if remote.exists(ctx, &remote_key).unwrap_or(false) {
            return Ok(());
        }

        // Upload (ignore errors - remote cache is best-effort)
        if let Err(e) = remote.upload(ctx, &remote_key, &object_path) {
            eprintln!("Warning: failed to push to remote cache: {e}");
        }

        Ok(())
    }

    /// Try to fetch an object from remote cache
    // Scaffolding for remote-pull: wired into the API surface but not yet
    // called from any read path. Intentional; tracked under remote-pull WIP.
    #[allow(dead_code)]
    pub(super) fn try_fetch_object_from_remote(&self, ctx: &crate::build_context::BuildContext, checksum: &str) -> Result<bool> {
        let Some(remote) = &self.remote else { return Ok(false) };

        let object_path = self.object_path(checksum);
        if object_path.exists() {
            return Ok(true);
        }

        let (prefix, rest) = checksum.split_at(super::CHECKSUM_PREFIX_LEN.min(checksum.len()));
        let remote_key = format!("objects/{prefix}/{rest}");
        let fetched = remote.download(ctx, &remote_key, &object_path)?;

        // Make fetched object read-only to prevent corruption via hardlinks
        if fetched {
            let mut perms = fs::metadata(&object_path)
                .context("Failed to read fetched object metadata")?
                .permissions();
            perms.set_readonly(true);
            fs::set_permissions(&object_path, perms)
                .context("Failed to set fetched object read-only")?;
        }

        Ok(fetched)
    }

    /// Try to push a descriptor to remote cache
    // Scaffolding for remote-pull (for paired fetch-after-push semantics).
    // Not yet called from any write path; tracked under remote-pull WIP.
    #[allow(dead_code)]
    #[allow(clippy::unnecessary_wraps)] // Result kept for API symmetry with try_fetch_*.
    pub(super) fn try_push_descriptor_to_remote(&self, ctx: &crate::build_context::BuildContext, descriptor_key: &str, data: &[u8]) -> Result<()> {
        let Some(remote) = &self.remote else { return Ok(()) };

        let remote_key = format!("descriptors/{descriptor_key}");
        if let Err(e) = remote.upload_bytes(ctx, &remote_key, data) {
            eprintln!("Warning: failed to push descriptor to remote cache: {e}");
        }

        Ok(())
    }

    /// Try to fetch a descriptor from remote cache.
    /// Scaffolding for remote-pull; not yet called from any read path.
    #[allow(dead_code)]
    pub(super) fn try_fetch_descriptor_from_remote(&self, ctx: &crate::build_context::BuildContext, descriptor_key: &str) -> Result<Option<Vec<u8>>> {
        let Some(remote) = &self.remote else { return Ok(None) };

        let remote_key = format!("descriptors/{descriptor_key}");
        let data = remote.download_bytes(ctx, &remote_key)?;
        if let Some(ref data) = data {
            // Store locally
            let path = self.descriptor_path(descriptor_key);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("Failed to create descriptor directory: {}", parent.display()))?;
            }
            fs::write(&path, data)
                .with_context(|| format!("Failed to write remote descriptor: {}", path.display()))?;
        }
        Ok(data)
    }
}
