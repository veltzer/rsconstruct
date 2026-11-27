use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use super::{CacheDescriptor, ExplainAction, ObjectStore, RebuildReason, TreeEntry};

impl ObjectStore {
    /// Restore outputs from a cache descriptor. Returns Ok(true) if restored.
    ///
    /// Both the descriptor and the objects it names are pulled from the
    /// remote cache on a local miss when `remote_pull` is enabled.
    pub fn restore_from_descriptor(
        &self,
        ctx: &crate::build_context::BuildContext,
        cache_key: &str,
        output_paths: &[PathBuf],
    ) -> Result<bool> {
        let Some(descriptor) = self.get_descriptor_pulling(ctx, cache_key) else {
            return Ok(false);
        };
        match descriptor {
            CacheDescriptor::Marker => Ok(true),
            CacheDescriptor::Blob { checksum, mode } => {
                let Some(output_path) = output_paths.first() else {
                    return Ok(true);
                };
                // Verify content like the Tree path does: a modified or
                // corrupted output must be re-restored, not reported OK.
                // Uses the mtime-cached path, matching needs_rebuild_descriptor
                // — a full re-read here was the third redundant hash of every
                // cached output byte on a no-op build.
                if output_path.exists() {
                    if let Some(existing) = Self::verify_checksum(ctx, output_path)
                        && existing == checksum
                    {
                        return Ok(true);
                    }
                    fs::remove_file(output_path).with_context(|| {
                        format!(
                            "Failed to remove stale cached file: {}",
                            output_path.display()
                        )
                    })?;
                }
                if !self.ensure_object(ctx, &checksum) {
                    return Ok(false);
                }
                if let Some(parent) = output_path.parent() {
                    fs::create_dir_all(parent).with_context(|| {
                        format!("Failed to create output directory: {}", parent.display())
                    })?;
                }
                self.restore_verified(&checksum, output_path, mode)
                    .with_context(|| {
                        format!("Failed to restore blob to: {}", output_path.display())
                    })
            }
            CacheDescriptor::Tree { entries } => {
                super::validate_tree_entries(&entries)?;
                for entry in &entries {
                    let file_path = super::safe_entry_path(&entry.path)?;
                    if Self::entry_is_current(ctx, entry) {
                        continue;
                    }
                    if entry.link.is_none() && !self.ensure_object(ctx, &entry.checksum) {
                        return Ok(false);
                    }
                    Self::remove_stale_entry(file_path)?;
                    if let Some(parent) = file_path.parent() {
                        fs::create_dir_all(parent).with_context(|| {
                            format!(
                                "Failed to create directory for tree restore: {}",
                                parent.display()
                            )
                        })?;
                    }
                    if let Some(target) = &entry.link {
                        crate::platform::symlink_file(Path::new(target), file_path).with_context(
                            || format!("Failed to restore symlink: {}", file_path.display()),
                        )?;
                        continue;
                    }
                    let intact = self
                        .restore_verified(&entry.checksum, file_path, entry.mode)
                        .with_context(|| {
                            format!("Failed to restore tree entry: {}", file_path.display())
                        })?;
                    if !intact {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
        }
    }

    /// Restore one file from its object and check that what came out is
    /// what the checksum names. `Ok(false)` when it is not: the object is
    /// damaged, so it and the file just restored from it are removed, and
    /// the product is built instead.
    ///
    /// An object is written to a temp file and renamed into place without
    /// being synced, so a power loss can leave an empty or short file
    /// under a valid name. Nothing looked at an object's content again:
    /// the damaged bytes were restored, the next build found the output
    /// did not match and restored the same bytes once more, and every
    /// build reported "restored" over an output that stayed wrong.
    fn restore_verified(&self, checksum: &str, path: &Path, mode: Option<u32>) -> Result<bool> {
        self.restore_file(checksum, path, mode)?;
        let content = fs::read(path)
            .with_context(|| format!("Failed to read back restored file: {}", path.display()))?;
        if Self::calculate_checksum_bytes(&content) == checksum {
            return Ok(true);
        }
        crate::output::warn(&format!(
            "cache object {checksum} is damaged (its content does not match its name); \
             removing it and rebuilding {}",
            path.display()
        ));
        fs::remove_file(path).with_context(|| format!("Failed to remove {}", path.display()))?;
        self.remove_object(checksum)?;
        Ok(false)
    }

    /// Whether a tree entry is on disk as recorded: a symbolic link with
    /// the recorded target, or a regular file with the recorded content. A
    /// link standing where a file was recorded is not current, whatever it
    /// points at, and neither is a file where a link was recorded.
    fn entry_is_current(ctx: &crate::build_context::BuildContext, entry: &TreeEntry) -> bool {
        let path = Path::new(&entry.path);
        entry.link.as_ref().map_or_else(
            || {
                fs::symlink_metadata(path).is_ok_and(|m| m.is_file())
                    && Self::verify_checksum(ctx, path).as_ref() == Some(&entry.checksum)
            },
            |target| super::is_link_to(path, target),
        )
    }

    /// Remove whatever stands where a tree entry is about to be restored:
    /// nothing, a file, a link, or a directory left by an earlier shape of
    /// the tree.
    fn remove_stale_entry(path: &Path) -> Result<()> {
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => {
                return Err(e).with_context(|| format!("Failed to stat {}", path.display()));
            }
        };
        if metadata.is_dir() {
            fs::remove_dir_all(path)
        } else {
            fs::remove_file(path)
        }
        .with_context(|| format!("Failed to remove stale cached entry: {}", path.display()))
    }

    /// Checksum an output for verification, consulting the persistent mtime
    /// cache so an unchanged file is not re-read.
    ///
    /// Output verification runs on every no-op build (classify, then again
    /// per level), so full-reading each output is what made cached trees
    /// like `.venv` catastrophically slow. `checksum_fast` degrades to the
    /// same full read when the mtime doesn't match, so the answer is
    /// identical — only the cost changes.
    fn verify_checksum(ctx: &crate::build_context::BuildContext, path: &Path) -> Option<String> {
        crate::checksum::checksum_output(ctx, path)
            .ok()
            .map(|(c, _)| c)
    }

    /// Check if a product needs rebuilding based on its descriptor.
    pub fn needs_rebuild_descriptor(
        &self,
        ctx: &crate::build_context::BuildContext,
        cache_key: &str,
        output_paths: &[PathBuf],
    ) -> bool {
        let Some(descriptor) = self.get_descriptor_pulling(ctx, cache_key) else {
            return true;
        };
        match descriptor {
            CacheDescriptor::Marker => false,
            CacheDescriptor::Blob { checksum, .. } => {
                // First output is content-verified against the descriptor,
                // consistent with the Tree path; extra outputs are
                // existence-checked only (their checksums aren't recorded).
                let content_ok = output_paths
                    .first()
                    .is_none_or(|p| Self::verify_checksum(ctx, p).as_ref() == Some(&checksum));
                !content_ok || output_paths.iter().skip(1).any(|p| !p.exists())
            }
            CacheDescriptor::Tree { entries } => {
                // Declared outputs are existence-checked too: a tree records
                // only what the previous run produced, so an output the
                // product now declares but the descriptor never saw would
                // otherwise go unnoticed.
                output_paths.iter().any(|p| !p.exists())
                    || entries.iter().any(|e| !Self::entry_is_current(ctx, e))
            }
        }
    }

    /// Check if outputs can be restored from a descriptor.
    ///
    /// Consults the remote cache on a local miss when `remote_pull` is
    /// enabled — which also warms the local store, so the restore that
    /// follows finds everything it needs.
    pub fn can_restore_descriptor(
        &self,
        ctx: &crate::build_context::BuildContext,
        cache_key: &str,
    ) -> bool {
        let Some(descriptor) = self.get_descriptor_pulling(ctx, cache_key) else {
            return false;
        };
        match descriptor {
            CacheDescriptor::Marker => true,
            CacheDescriptor::Blob { checksum, .. } => self.ensure_object(ctx, &checksum),
            CacheDescriptor::Tree { entries } => {
                // Not short-circuiting: every object must be pulled, not just
                // up to the first miss, or the restore that follows finds a
                // half-warmed local store. clippy suggests `all`, which is
                // exactly wrong here — its own lint note warns that `all`
                // short-circuits and changes semantics when the closure has
                // side effects, and pulling an object is the side effect.
                let mut all_present = true;
                for e in entries {
                    if e.link.is_none() && !self.ensure_object(ctx, &e.checksum) {
                        all_present = false;
                    }
                }
                all_present
            }
        }
    }

    /// Explain what action will be taken based on descriptor state.
    ///
    /// Read-only: this backs `status`, `product show`, `build --dry-run` and
    /// `--explain`, none of which may change the cache. The descriptor is
    /// looked up with `get_descriptor_peeking`, which consults the remote
    /// but does not write what it finds to `.rsconstruct/descriptors/`;
    /// objects are checked with `object_available`, which likewise never
    /// downloads. The verdict must match `needs_rebuild_descriptor` and
    /// `can_restore_descriptor` — a "Skip" here where the build would
    /// restore is a lie to the user.
    pub fn explain_descriptor(
        &self,
        ctx: &crate::build_context::BuildContext,
        descriptor_key: &str,
        output_paths: &[PathBuf],
        force: bool,
    ) -> ExplainAction {
        if force {
            return ExplainAction::Rebuild(RebuildReason::Force);
        }
        let Some(descriptor) = self.get_descriptor_peeking(ctx, descriptor_key) else {
            return ExplainAction::Rebuild(RebuildReason::NoCacheEntry);
        };
        match descriptor {
            CacheDescriptor::Marker => ExplainAction::Skip,
            CacheDescriptor::Blob { checksum, .. } => {
                // Same tiered verification as needs_rebuild_descriptor: the
                // first output is content-verified (its checksum is the
                // descriptor), extras are existence-checked only — --explain
                // must never disagree with what the build would do.
                for (i, p) in output_paths.iter().enumerate() {
                    let needs_restore = if i == 0 {
                        !p.exists() || Self::verify_checksum(ctx, p).as_ref() != Some(&checksum)
                    } else {
                        !p.exists()
                    };
                    if needs_restore {
                        let display = p.display().to_string();
                        if self.object_available(ctx, &checksum) {
                            return ExplainAction::Restore(RebuildReason::OutputMissing(display));
                        }
                        return ExplainAction::Rebuild(RebuildReason::OutputMissing(display));
                    }
                }
                ExplainAction::Skip
            }
            CacheDescriptor::Tree { entries } => {
                // Declared outputs are existence-checked exactly as in
                // needs_rebuild_descriptor: a tree records only what the
                // previous run produced, so an output the product declares
                // but the descriptor never saw is otherwise invisible here —
                // and `status` would say "up-to-date" where the build acts.
                // The build then restores when every recorded object can be
                // had (can_restore_descriptor), and rebuilds otherwise.
                if let Some(missing) = output_paths.iter().find(|p| !p.exists()) {
                    let display = missing.display().to_string();
                    if entries
                        .iter()
                        .all(|e| e.link.is_some() || self.object_available(ctx, &e.checksum))
                    {
                        return ExplainAction::Restore(RebuildReason::OutputMissing(display));
                    }
                    return ExplainAction::Rebuild(RebuildReason::OutputMissing(display));
                }
                for entry in &entries {
                    if !Self::entry_is_current(ctx, entry) {
                        if entry.link.is_some() || self.object_available(ctx, &entry.checksum) {
                            return ExplainAction::Restore(RebuildReason::OutputMissing(
                                entry.path.clone(),
                            ));
                        }
                        return ExplainAction::Rebuild(RebuildReason::OutputMissing(
                            entry.path.clone(),
                        ));
                    }
                }
                ExplainAction::Skip
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build_context::BuildContext;

    /// Marker descriptors are the checker fast-path: a stored PASS must
    /// never trigger a rebuild, and a missing descriptor always must.
    #[test]
    fn marker_never_rebuilds_missing_descriptor_always_does() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = ObjectStore::new_in(tmp.path());
        let ctx = BuildContext::new();
        // The mtime cache is CWD-relative (.rsconstruct/mtime.redb); disable it
        // so parallel unit tests do not share persistent state.
        ctx.set_mtime_check(false);

        assert!(store.needs_rebuild_descriptor(&ctx, "absent99", &[]));
        assert!(!store.can_restore_descriptor(&ctx, "absent99"));

        store.store_marker(&ctx, "cafe1234").unwrap();
        assert!(!store.needs_rebuild_descriptor(&ctx, "cafe1234", &[]));
        assert!(store.can_restore_descriptor(&ctx, "cafe1234"));
    }

    /// An object whose content no longer matches its name (emptied by a
    /// power loss, say) is not restored: the restore reports that it could
    /// not be done, the damaged object and the file made from it are gone,
    /// and once the product is built again the entry restores normally.
    #[test]
    fn a_damaged_object_is_dropped_instead_of_restored() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = ObjectStore::new_in(tmp.path());
        let ctx = BuildContext::new();
        ctx.set_mtime_check(false);

        let out = tmp.path().join("out.txt");
        fs::write(&out, b"the real content").unwrap();
        store.store_blob_descriptor(&ctx, "feed0001", &out).unwrap();
        let checksum = ObjectStore::calculate_checksum_bytes(b"the real content");

        // Damage the object the way a lost write does, and lose the output.
        let object = store.object_path(&checksum);
        crate::platform::set_permissions_mode(&object, 0o644).unwrap();
        fs::write(&object, b"").unwrap();
        fs::remove_file(&out).unwrap();

        let outputs = std::slice::from_ref(&out);
        assert!(
            !store
                .restore_from_descriptor(&ctx, "feed0001", outputs)
                .unwrap()
        );
        assert!(
            !out.exists(),
            "the damaged bytes must not be left as output"
        );
        assert!(!store.has_object(&checksum), "the damaged object must go");
        assert!(!store.can_restore_descriptor(&ctx, "feed0001"));

        // Built again, the entry is whole and restores.
        fs::write(&out, b"the real content").unwrap();
        store.store_blob_descriptor(&ctx, "feed0001", &out).unwrap();
        fs::remove_file(&out).unwrap();
        assert!(
            store
                .restore_from_descriptor(&ctx, "feed0001", outputs)
                .unwrap()
        );
        assert_eq!(fs::read(&out).unwrap(), b"the real content");
    }

    /// Blob verification is tiered: the first output is content-verified
    /// (its checksum is the descriptor), the remaining outputs are
    /// existence-checked only — their checksums were never recorded.
    #[test]
    fn blob_rebuild_verification_is_tiered() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = ObjectStore::new_in(tmp.path());
        let ctx = BuildContext::new();
        // The mtime cache is CWD-relative (.rsconstruct/mtime.redb); disable it
        // so parallel unit tests do not share persistent state.
        ctx.set_mtime_check(false);
        let key = "beef5678";

        let first = tmp.path().join("first.out");
        let second = tmp.path().join("second.out");
        fs::write(&first, b"primary").unwrap();
        store.store_blob_descriptor(&ctx, key, &first).unwrap();

        let outputs = vec![first.clone(), second.clone()];
        assert!(
            store.needs_rebuild_descriptor(&ctx, key, &outputs),
            "second output missing → rebuild"
        );

        fs::write(&second, b"anything at all").unwrap();
        assert!(
            !store.needs_rebuild_descriptor(&ctx, key, &outputs),
            "extra outputs are existence-checked only"
        );

        fs::write(&first, b"tampered").unwrap();
        assert!(
            store.needs_rebuild_descriptor(&ctx, key, &outputs),
            "first output is content-verified"
        );
    }

    /// A corrupted output must be re-materialized from the cache, not
    /// reported OK; a descriptor whose object is gone must report false so
    /// the caller falls back to building.
    #[test]
    fn restore_replaces_corrupted_output_and_reports_missing_object() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = ObjectStore::new_in(tmp.path());
        let ctx = BuildContext::new();
        // The mtime cache is CWD-relative (.rsconstruct/mtime.redb); disable it
        // so parallel unit tests do not share persistent state.
        ctx.set_mtime_check(false);
        let key = "feed9abc";

        let out = tmp.path().join("out.txt");
        fs::write(&out, b"cached bytes").unwrap();
        store.store_blob_descriptor(&ctx, key, &out).unwrap();

        fs::write(&out, b"corrupted").unwrap();
        assert!(
            store
                .restore_from_descriptor(&ctx, key, std::slice::from_ref(&out))
                .unwrap()
        );
        assert_eq!(
            fs::read(&out).unwrap(),
            b"cached bytes",
            "restore must replace corrupted content with the cached bytes"
        );

        // Remove the object behind the descriptor: restore must decline.
        let checksum = ObjectStore::calculate_checksum_bytes(&fs::read(&out).unwrap());
        fs::remove_file(store.object_path(&checksum)).unwrap();
        fs::remove_file(&out).unwrap();
        assert!(
            !store
                .restore_from_descriptor(&ctx, key, std::slice::from_ref(&out))
                .unwrap(),
            "no object → cannot restore, caller must build"
        );
    }

    /// Tree descriptors content-verify every entry — corrupting any one
    /// file flags a rebuild, and restore puts the recorded bytes back.
    #[test]
    fn tree_verifies_and_restores_every_entry() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = ObjectStore::new_in(tmp.path());
        let ctx = BuildContext::new();
        // The mtime cache is CWD-relative (.rsconstruct/mtime.redb); disable it
        // so parallel unit tests do not share persistent state.
        ctx.set_mtime_check(false);
        let key = "dead4321";

        let outdir = tmp.path().join("outdir");
        fs::create_dir_all(&outdir).unwrap();
        fs::write(outdir.join("a.txt"), b"alpha").unwrap();
        fs::write(outdir.join("b.txt"), b"beta").unwrap();
        let dirs = [std::sync::Arc::new(outdir.clone())];
        store
            .store_tree_descriptor(&ctx, key, &dirs, &[], &|_| false)
            .unwrap();

        assert!(!store.needs_rebuild_descriptor(&ctx, key, &[]));

        fs::write(outdir.join("b.txt"), b"tampered").unwrap();
        assert!(
            store.needs_rebuild_descriptor(&ctx, key, &[]),
            "any corrupted tree entry must flag a rebuild"
        );

        assert!(store.restore_from_descriptor(&ctx, key, &[]).unwrap());
        assert_eq!(fs::read(outdir.join("b.txt")).unwrap(), b"beta");
        assert!(!store.needs_rebuild_descriptor(&ctx, key, &[]));
    }

    /// `explain_descriptor` must agree with `needs_rebuild_descriptor` on a
    /// tree whose *declared* output is gone from disk. The Tree arm used to
    /// check only the recorded entries, so `status`, `product show` and
    /// `build --dry-run` reported "up-to-date" where the build would act.
    #[test]
    fn explain_tree_sees_a_missing_declared_output() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = ObjectStore::new_in(tmp.path());
        let ctx = BuildContext::new();
        // The mtime cache is CWD-relative (.rsconstruct/mtime.redb); disable it
        // so parallel unit tests do not share persistent state.
        ctx.set_mtime_check(false);
        let key = "ace00011";

        let outdir = tmp.path().join("outdir");
        fs::create_dir_all(&outdir).unwrap();
        fs::write(outdir.join("a.txt"), b"alpha").unwrap();
        let dirs = [std::sync::Arc::new(outdir.clone())];
        store
            .store_tree_descriptor(&ctx, key, &dirs, &[], &|_| false)
            .unwrap();

        // A file the product declares now but the recorded tree never saw.
        let declared = tmp.path().join("declared.txt");
        let outputs = vec![declared.clone()];
        assert!(
            store.needs_rebuild_descriptor(&ctx, key, &outputs),
            "the build acts on the missing declared output"
        );
        assert!(
            matches!(
                store.explain_descriptor(&ctx, key, &outputs, false),
                ExplainAction::Restore(RebuildReason::OutputMissing(ref p)) if p == &declared.display().to_string()
            ),
            "explain must report the missing declared output, not Skip"
        );

        fs::write(&declared, b"present").unwrap();
        assert!(!store.needs_rebuild_descriptor(&ctx, key, &outputs));
        assert!(matches!(
            store.explain_descriptor(&ctx, key, &outputs, false),
            ExplainAction::Skip
        ));

        // A recorded entry gone from disk is reported the same way.
        fs::remove_file(outdir.join("a.txt")).unwrap();
        assert!(matches!(
            store.explain_descriptor(&ctx, key, &outputs, false),
            ExplainAction::Restore(RebuildReason::OutputMissing(_))
        ));
    }

    /// Explaining is read-only: a descriptor that only the remote holds is
    /// consulted (so the verdict is "restore", as the build would) but not
    /// written into the local `descriptors/` store. The build path's
    /// `get_descriptor_pulling` does persist it — `status`, `product show`
    /// and `build --dry-run` went through that and populated the cache.
    #[test]
    fn explain_does_not_write_a_pulled_descriptor_locally() {
        let tmp = tempfile::TempDir::new().unwrap();
        let remote_dir = tmp.path().join("remote");
        let ctx = BuildContext::new();
        ctx.set_mtime_check(false);
        let key = "0dde5678";

        let out = tmp.path().join("out.txt");
        fs::write(&out, b"produced bytes").unwrap();
        {
            let producer =
                ObjectStore::new_with_remote(&tmp.path().join("a"), "a.redb", &remote_dir);
            producer.store_blob_descriptor(&ctx, key, &out).unwrap();
        }
        fs::remove_file(&out).unwrap();

        let consumer_dir = tmp.path().join("b");
        let consumer = ObjectStore::new_with_remote(&consumer_dir, "b.redb", &remote_dir);
        assert!(consumer.get_descriptor(key).is_none(), "cold local cache");

        let verdict = consumer.explain_descriptor(&ctx, key, std::slice::from_ref(&out), false);
        assert!(
            matches!(
                verdict,
                ExplainAction::Restore(RebuildReason::OutputMissing(_))
            ),
            "the remote descriptor and object make this a restore: {verdict:?}"
        );
        assert!(
            !consumer.has_descriptor(key),
            "explaining must not populate the local descriptor store"
        );
        assert!(
            consumer.descriptor_rows().unwrap().is_empty(),
            "no descriptor was stored by the read-only lookup"
        );
        assert!(
            !consumer.has_object(&ObjectStore::calculate_checksum_bytes(b"produced bytes")),
            "nor was the object downloaded"
        );

        // The build path is the one that warms the local store.
        assert!(consumer.can_restore_descriptor(&ctx, key));
        assert!(consumer.has_descriptor(key));
    }
}
