use anyhow::{Context, Result};

use super::ObjectStore;

impl ObjectStore {
    /// Try to push an object to remote cache. A failure is a warning, not an
    /// error (the push is best-effort); the result says whether the remote
    /// now holds the object, so the caller does not publish a descriptor
    /// naming an object that is not there.
    /// The remote always stores the original (uncompressed) content so
    /// machines with different local `compression` settings interoperate.
    pub(super) fn try_push_object_to_remote(
        &self,
        ctx: &crate::build_context::BuildContext,
        checksum: &str,
    ) -> bool {
        let Some(remote) = &self.remote else {
            return false;
        };

        if !self.has_object(checksum) {
            return false;
        }

        let remote_key = super::remote_object_key(checksum);

        // Check if already exists remotely (avoid redundant uploads). A
        // failed check is reported like a failed upload: the push is
        // best-effort, but a broken remote (expired credentials, denied
        // bucket) must never pass for "not there yet" without a word.
        match remote.exists(ctx, &remote_key) {
            Ok(true) => return true,
            Ok(false) => {}
            Err(e) => {
                crate::output::warn(&format!(
                    "failed to check remote cache for {remote_key}: {e}"
                ));
                return false;
            }
        }

        match self.read_object(checksum) {
            Ok(content) => match remote.upload_bytes(ctx, &remote_key, &content) {
                Ok(()) => true,
                Err(e) => {
                    crate::output::warn(&format!("failed to push to remote cache: {e}"));
                    false
                }
            },
            Err(e) => {
                crate::output::warn(&format!("failed to read object for remote push: {e}"));
                false
            }
        }
    }

    /// Whether the object is available locally, fetching it from the remote
    /// cache first if pull is enabled and it is missing.
    ///
    /// This is the read-path entry point for remote pull. Restore decisions
    /// go through it rather than through `has_object` directly, so a
    /// populated remote bucket can actually satisfy a restore — before this,
    /// push worked but every fetch path was dead code, making a configured
    /// remote a write-only feature.
    pub(super) fn ensure_object(
        &self,
        ctx: &crate::build_context::BuildContext,
        checksum: &str,
    ) -> bool {
        if self.has_object(checksum) {
            return true;
        }
        if !self.remote_pull {
            return false;
        }
        // A remote miss (or a broken remote) is not an error: it degrades to
        // a local rebuild, which is always correct. A *corrupt* remote object
        // is different — try_fetch_object_from_remote rejects it rather than
        // poisoning the local store, and we report it here so a bad bucket
        // doesn't fail silently forever.
        match self.try_fetch_object_from_remote(ctx, checksum) {
            Ok(fetched) => fetched,
            Err(e) => {
                crate::output::warn(&format!("failed to fetch from remote cache: {e}"));
                false
            }
        }
    }

    /// Whether the object could be obtained — locally, or from the remote
    /// without downloading it now.
    ///
    /// Used by the reporting paths (`--explain`, `product show`, status),
    /// which must agree with what a real build would do but must not have
    /// the side effect of populating the local cache. `ensure_object` is the
    /// build-path counterpart that actually fetches.
    pub(super) fn object_available(
        &self,
        ctx: &crate::build_context::BuildContext,
        checksum: &str,
    ) -> bool {
        if self.has_object(checksum) {
            return true;
        }
        if !self.remote_pull {
            return false;
        }
        let Some(remote) = &self.remote else {
            return false;
        };
        let remote_key = super::remote_object_key(checksum);
        // Same policy as `ensure_object`: a broken remote degrades to "not
        // available" (the build rebuilds locally, which is always correct),
        // but it is reported, never swallowed.
        match remote.exists(ctx, &remote_key) {
            Ok(available) => available,
            Err(e) => {
                crate::output::warn(&format!(
                    "failed to check remote cache for {remote_key}: {e}"
                ));
                false
            }
        }
    }

    /// Try to fetch an object from remote cache
    pub(super) fn try_fetch_object_from_remote(
        &self,
        ctx: &crate::build_context::BuildContext,
        checksum: &str,
    ) -> Result<bool> {
        let Some(remote) = &self.remote else {
            return Ok(false);
        };

        if self.has_object(checksum) {
            return Ok(true);
        }

        let remote_key = super::remote_object_key(checksum);
        let Some(bytes) = remote.download_bytes(ctx, &remote_key)? else {
            return Ok(false);
        };

        // Verify before admitting into the local content-addressed store —
        // a corrupt or malicious remote must not poison it.
        let actual = Self::calculate_checksum_bytes(&bytes);
        if actual != checksum {
            anyhow::bail!(
                "Remote cache object {remote_key} is corrupt: content hashes to {actual}, expected {checksum}"
            );
        }

        // store_object writes atomically and applies the local storage format.
        self.store_object(&bytes)?;
        Ok(true)
    }

    /// Try to push a descriptor to remote cache
    pub(super) fn try_push_descriptor_to_remote(
        &self,
        ctx: &crate::build_context::BuildContext,
        descriptor_key: &str,
        data: &[u8],
    ) {
        let Some(remote) = &self.remote else {
            return;
        };

        let remote_key = format!("descriptors/{descriptor_key}");
        if let Err(e) = remote.upload_bytes(ctx, &remote_key, data) {
            crate::output::warn(&format!("failed to push descriptor to remote cache: {e}"));
        }
    }

    /// Try to fetch a descriptor from remote cache, caching it locally.
    ///
    /// The fetched bytes are parsed before being written: a descriptor that
    /// doesn't deserialize would otherwise land in the local store and
    /// permanently break `cache trim`, which fails closed on a parse error.
    /// Unlike objects, a descriptor is not content-addressed, so parsing is
    /// the only integrity check available.
    pub(super) fn try_fetch_descriptor_from_remote(
        &self,
        ctx: &crate::build_context::BuildContext,
        descriptor_key: &str,
    ) -> Result<Option<super::CacheDescriptor>> {
        let Some(descriptor) = self.peek_descriptor_from_remote(ctx, descriptor_key)? else {
            return Ok(None);
        };
        // Reuse store_descriptor rather than fs::write so the remote path
        // gets the same temp-then-rename atomicity as the local one.
        self.store_descriptor(descriptor_key, &descriptor)?;
        Ok(Some(descriptor))
    }

    /// Download and validate a descriptor from the remote cache without
    /// writing anything locally. The read-only inspection commands
    /// (`status`, `product show`, `build --dry-run`, `--explain`) go through
    /// this so that looking at the cache never changes it; the build path
    /// uses `try_fetch_descriptor_from_remote`, which persists the result.
    pub(super) fn peek_descriptor_from_remote(
        &self,
        ctx: &crate::build_context::BuildContext,
        descriptor_key: &str,
    ) -> Result<Option<super::CacheDescriptor>> {
        let Some(remote) = &self.remote else {
            return Ok(None);
        };

        let remote_key = format!("descriptors/{descriptor_key}");
        let Some(data) = remote.download_bytes(ctx, &remote_key)? else {
            return Ok(None);
        };

        let descriptor: super::CacheDescriptor = serde_json::from_slice(&data)
            .with_context(|| format!("Remote cache descriptor {remote_key} is malformed"))?;

        // A fetched descriptor is a write primitive (restore) and a delete
        // primitive (stale-output cleanup): validate every entry path before
        // admitting it to the local store. Production entries are always
        // project-root-relative.
        // Every checksum is validated too. It becomes a path below
        // `objects/` (`object_path`), so one holding a `/` would address a
        // file outside the store, and one with a multi-byte first character
        // would panic the two-character shard split.
        let checksums: Vec<&str> = match &descriptor {
            super::CacheDescriptor::Marker => Vec::new(),
            super::CacheDescriptor::Blob { checksum, .. } => vec![checksum.as_str()],
            // A symlink entry names no object.
            super::CacheDescriptor::Tree { entries } => entries
                .iter()
                .filter(|e| e.link.is_none())
                .map(|e| e.checksum.as_str())
                .collect(),
        };
        for checksum in checksums {
            if !super::is_object_checksum(checksum) {
                anyhow::bail!(
                    "Remote cache descriptor {remote_key} contains '{checksum}', which is not a SHA-256 checksum"
                );
            }
        }
        if let super::CacheDescriptor::Tree { entries } = &descriptor {
            super::validate_tree_entries(entries)
                .with_context(|| format!("Remote cache descriptor {remote_key} rejected"))?;
            for entry in entries {
                let path = super::safe_entry_path(&entry.path)
                    .with_context(|| format!("Remote cache descriptor {remote_key} rejected"))?;
                if path.is_absolute() {
                    anyhow::bail!(
                        "Remote cache descriptor {remote_key} contains absolute entry path '{}'",
                        entry.path
                    );
                }
            }
        }

        Ok(Some(descriptor))
    }
}

#[cfg(test)]
mod tests {
    use super::super::ObjectStore;
    use crate::build_context::BuildContext;
    use std::fs;

    /// A remote whose every existence check fails, the way `aws` does with
    /// expired credentials or a denied bucket.
    struct BrokenRemote;

    impl crate::remote_cache::RemoteCache for BrokenRemote {
        fn exists(&self, _ctx: &BuildContext, key: &str) -> anyhow::Result<bool> {
            anyhow::bail!("S3 head-object failed for {key}: (ExpiredToken)")
        }
        fn upload(
            &self,
            _ctx: &BuildContext,
            _key: &str,
            _src: &std::path::Path,
        ) -> anyhow::Result<()> {
            Ok(())
        }
        fn download_bytes(
            &self,
            _ctx: &BuildContext,
            _key: &str,
        ) -> anyhow::Result<Option<Vec<u8>>> {
            Ok(None)
        }
    }

    /// A failing existence check is reported and degrades — the push is
    /// skipped and the object counts as unavailable — but never fails the
    /// store that is recording a finished product. Before, the error was
    /// collapsed to "absent" with `unwrap_or(false)` and nobody heard of it.
    #[test]
    fn a_failing_exists_check_degrades_without_failing_the_store() {
        let tmp = tempfile::TempDir::new().unwrap();
        let ctx = BuildContext::new();
        ctx.set_mtime_check(false);
        let mut store =
            ObjectStore::new_with_remote(&tmp.path().join("a"), "a.redb", &tmp.path().join("r"));
        store.remote = Some(Box::new(BrokenRemote));

        let out = tmp.path().join("out.txt");
        fs::write(&out, b"produced bytes").unwrap();
        store.store_blob_descriptor(&ctx, "0bcdbeef", &out).unwrap();

        let checksum = ObjectStore::calculate_checksum_bytes(b"never stored");
        assert!(!store.object_available(&ctx, &checksum));
    }

    /// A remote that takes descriptors but fails every object upload, and
    /// records the keys it was asked to store.
    struct ObjectsFailRemote {
        uploads: std::sync::Arc<parking_lot::Mutex<Vec<String>>>,
    }

    impl crate::remote_cache::RemoteCache for ObjectsFailRemote {
        fn exists(&self, _ctx: &BuildContext, _key: &str) -> anyhow::Result<bool> {
            Ok(false)
        }
        fn upload(
            &self,
            _ctx: &BuildContext,
            key: &str,
            _src: &std::path::Path,
        ) -> anyhow::Result<()> {
            self.uploads.lock().push(key.to_string());
            anyhow::ensure!(!key.starts_with("objects/"), "upload of {key} refused");
            Ok(())
        }
        fn download_bytes(
            &self,
            _ctx: &BuildContext,
            _key: &str,
        ) -> anyhow::Result<Option<Vec<u8>>> {
            Ok(None)
        }
    }

    /// A descriptor is published only when the objects it names reached
    /// the remote: alone, it is an entry no machine can restore. A marker
    /// names no object and is always published.
    #[test]
    fn a_descriptor_is_not_pushed_without_its_objects() {
        let tmp = tempfile::TempDir::new().unwrap();
        let ctx = BuildContext::new();
        ctx.set_mtime_check(false);
        let uploads = std::sync::Arc::new(parking_lot::Mutex::new(Vec::new()));
        let mut store =
            ObjectStore::new_with_remote(&tmp.path().join("a"), "a.redb", &tmp.path().join("r"));
        store.remote = Some(Box::new(ObjectsFailRemote {
            uploads: std::sync::Arc::clone(&uploads),
        }));

        let out = tmp.path().join("out.txt");
        fs::write(&out, b"produced bytes").unwrap();
        store.store_blob_descriptor(&ctx, "0bcdbeef", &out).unwrap();
        store.store_marker(&ctx, "0marker0").unwrap();

        let uploads = uploads.lock();
        assert!(
            uploads.iter().any(|key| key.starts_with("objects/")),
            "the object push was attempted: {uploads:?}"
        );
        assert!(
            !uploads.contains(&"descriptors/0bcdbeef".to_string()),
            "{uploads:?}"
        );
        assert!(
            uploads.contains(&"descriptors/0marker0".to_string()),
            "{uploads:?}"
        );
    }

    /// The whole point of finding 12's remote-cache item: a machine with a
    /// cold local cache must be able to restore from what another machine
    /// pushed. Before this, push worked but every fetch path was dead code,
    /// so a populated bucket could never satisfy a restore.
    #[test]
    fn a_populated_remote_satisfies_a_cold_restore() {
        let tmp = tempfile::TempDir::new().unwrap();
        let remote_dir = tmp.path().join("remote");
        let ctx = BuildContext::new();
        ctx.set_mtime_check(false);
        let key = "0bcd1234";

        let out = tmp.path().join("out.txt");
        fs::write(&out, b"produced bytes").unwrap();

        // Machine A builds and pushes.
        {
            let producer =
                ObjectStore::new_with_remote(&tmp.path().join("a"), "a.redb", &remote_dir);
            producer.store_blob_descriptor(&ctx, key, &out).unwrap();
        }

        // Machine B has an empty local store and no output on disk.
        fs::remove_file(&out).unwrap();
        let consumer = ObjectStore::new_with_remote(&tmp.path().join("b"), "b.redb", &remote_dir);

        assert!(
            consumer.get_descriptor(key).is_none(),
            "consumer must start with a cold local cache"
        );
        assert!(
            consumer.can_restore_descriptor(&ctx, key),
            "a populated remote must be able to satisfy the restore"
        );
        assert!(
            consumer
                .restore_from_descriptor(&ctx, key, std::slice::from_ref(&out))
                .unwrap()
        );
        assert_eq!(fs::read(&out).unwrap(), b"produced bytes");
    }

    /// With pull disabled the remote is invisible to the read path, even
    /// though the bytes are sitting right there.
    #[test]
    fn pull_disabled_ignores_a_populated_remote() {
        let tmp = tempfile::TempDir::new().unwrap();
        let remote_dir = tmp.path().join("remote");
        let ctx = BuildContext::new();
        ctx.set_mtime_check(false);
        let key = "0bcd5678";

        let out = tmp.path().join("out.txt");
        fs::write(&out, b"produced bytes").unwrap();
        {
            let producer =
                ObjectStore::new_with_remote(&tmp.path().join("a"), "a.redb", &remote_dir);
            producer.store_blob_descriptor(&ctx, key, &out).unwrap();
        }
        fs::remove_file(&out).unwrap();

        let mut consumer =
            ObjectStore::new_with_remote(&tmp.path().join("b"), "b.redb", &remote_dir);
        consumer.remote_pull = false;
        assert!(!consumer.can_restore_descriptor(&ctx, key));
        assert!(
            !consumer
                .restore_from_descriptor(&ctx, key, std::slice::from_ref(&out))
                .unwrap()
        );
    }

    /// Tree descriptors must round-trip too — every entry's object has to
    /// come down, not just the first.
    #[test]
    fn remote_pull_restores_every_tree_entry() {
        let ctx = BuildContext::new();
        ctx.set_mtime_check(false);
        let key = "0bcd9abc";

        // Tree entry paths must be relative (production entries are always
        // project-root-relative; the remote-fetch boundary rejects absolute
        // ones), so everything lives under a cwd-relative dir — tempfile
        // can't be used because it canonicalizes to an absolute path, and
        // the stores must share a filesystem with the tree dir for the
        // hardlink restore.
        let base = std::path::PathBuf::from(format!(
            "target/test-tmp/remote-pull-{}",
            std::process::id()
        ));
        let remote_dir = base.join("remote");
        let outdir = base.join("outdir");
        fs::create_dir_all(&outdir).unwrap();
        fs::write(outdir.join("a.txt"), b"alpha").unwrap();
        fs::write(outdir.join("b.txt"), b"beta").unwrap();
        let dirs = [std::sync::Arc::new(outdir.clone())];
        {
            let producer = ObjectStore::new_with_remote(&base.join("a"), "a.redb", &remote_dir);
            producer
                .store_tree_descriptor(&ctx, key, &dirs, &[], &|_| false)
                .unwrap();
        }

        fs::remove_dir_all(&outdir).unwrap();
        let consumer = ObjectStore::new_with_remote(&base.join("b"), "b.redb", &remote_dir);
        assert!(consumer.restore_from_descriptor(&ctx, key, &[]).unwrap());
        assert_eq!(fs::read(outdir.join("a.txt")).unwrap(), b"alpha");
        assert_eq!(fs::read(outdir.join("b.txt")).unwrap(), b"beta");
        let _ = fs::remove_dir_all(&base);
    }

    /// A corrupt remote must never poison the local content-addressed store.
    #[test]
    fn corrupt_remote_object_is_rejected() {
        let tmp = tempfile::TempDir::new().unwrap();
        let remote_dir = tmp.path().join("remote");
        let ctx = BuildContext::new();
        ctx.set_mtime_check(false);
        let key = "0bcddead";

        let out = tmp.path().join("out.txt");
        fs::write(&out, b"produced bytes").unwrap();
        let checksum = {
            let producer =
                ObjectStore::new_with_remote(&tmp.path().join("a"), "a.redb", &remote_dir);
            producer.store_blob_descriptor(&ctx, key, &out).unwrap();
            ObjectStore::calculate_checksum_bytes(b"produced bytes")
        };

        // Tamper with the remote copy of the object. Objects are stored
        // read-only, so replace the file rather than writing over it.
        let (prefix, rest) = checksum.split_at(super::super::CHECKSUM_PREFIX_LEN);
        let remote_object = remote_dir.join("objects").join(prefix).join(rest);
        fs::remove_file(&remote_object).unwrap();
        fs::write(&remote_object, b"tampered bytes").unwrap();

        fs::remove_file(&out).unwrap();
        let consumer = ObjectStore::new_with_remote(&tmp.path().join("b"), "b.redb", &remote_dir);
        assert!(
            !consumer
                .restore_from_descriptor(&ctx, key, std::slice::from_ref(&out))
                .unwrap(),
            "a corrupt remote object must not be admitted; caller falls back to building"
        );
        assert!(
            !consumer.has_object(&checksum),
            "the corrupt bytes must not land in the local store"
        );
    }

    /// A remote descriptor's checksum becomes a path below the objects
    /// directory, so only the exact form of a SHA-256 digest is admitted.
    #[test]
    fn only_sha256_hex_counts_as_an_object_checksum() {
        let real = ObjectStore::calculate_checksum_bytes(b"x");
        assert!(super::super::is_object_checksum(&real));
        for bad in [
            "",
            "ab",
            "xx/etc/passwd",
            "../../../../etc/passwd",
            &real.to_uppercase(),
            &format!("é{}", &real[2..]),
            &format!("{real}0"),
        ] {
            assert!(
                !super::super::is_object_checksum(bad),
                "{bad:?} must be rejected"
            );
        }
    }
}
