//! Cache descriptors: one per cached product, keyed by its descriptor key,
//! saying what its outputs were (a marker, one blob, or a tree of blobs).
//!
//! They live in the `descriptors` table of the store's redb database. They
//! used to be one small JSON file each under `.rsconstruct/descriptors/`,
//! which cost every build an open, a read and a close per product — 18k of
//! each for a build of `teaching-slides` that changed nothing, and most of
//! a cold-cache build's time, since each was a separate random read.
//!
//! Descriptor writes come one per built product, so a durable commit (an
//! fsync) for each would make a build of many cheap products wait on the
//! disk. Most writes are committed without one instead
//! (`Durability::None`): redb keeps them, consistent, and a later durable
//! commit persists them all. A durable commit is made at least every
//! `DURABLE_COMMIT_INTERVAL` while writes keep coming, and by `persist`,
//! which runs when the store is dropped at the end of every command. A
//! process killed outright loses at most the last interval's entries,
//! which only means rebuilding those products.

use anyhow::{Context, Result};
use redb::ReadableDatabase;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::{CacheDescriptor, DESCRIPTORS_TABLE, ObjectStore, TreeEntry, TreeItem, walk_tree};

/// The longest a run of descriptor writes goes without a durable commit.
const DURABLE_COMMIT_INTERVAL: Duration = Duration::from_secs(1);

/// See `ObjectStore::descriptor_sync`.
#[derive(Default)]
pub struct DescriptorSync {
    /// When a descriptor write last ended in a durable commit; `None` before
    /// the first, so the first write of a run is durable.
    last_durable: Option<Instant>,
    /// Whether a descriptor write has been committed without reaching the
    /// disk since then.
    unsynced: bool,
}

impl ObjectStore {
    /// Run `edit` on the descriptor table in one write transaction, committed
    /// durably only when the last durable commit is older than
    /// `DURABLE_COMMIT_INTERVAL` (see the module doc).
    pub(super) fn write_descriptors(
        &self,
        edit: impl FnOnce(&mut redb::Table<'_, &str, &[u8]>) -> Result<()>,
    ) -> Result<()> {
        let mut txn = crate::errors::ctx(
            self.db.begin_write(),
            "Failed to begin a write to the cache database",
        )?;
        let mut sync = self.descriptor_sync.lock().unwrap();
        let durable = sync
            .last_durable
            .is_none_or(|at| at.elapsed() >= DURABLE_COMMIT_INTERVAL);
        if !durable {
            txn.set_durability(redb::Durability::None)
                .context("Failed to defer the cache database write")?;
        }
        {
            let mut table = txn
                .open_table(DESCRIPTORS_TABLE)
                .context("Failed to open the cache descriptor table")?;
            edit(&mut table)?;
        }
        crate::errors::ctx(txn.commit(), "Failed to commit cache descriptors")?;
        if durable {
            sync.last_durable = Some(Instant::now());
            sync.unsynced = false;
        } else {
            sync.unsynced = true;
        }
        Ok(())
    }

    /// Make every descriptor write so far durable. A no-op when they all are
    /// already, so a command that wrote nothing costs no fsync.
    pub fn persist(&self) -> Result<()> {
        if !self.descriptor_sync.lock().unwrap().unsynced {
            return Ok(());
        }
        // The write transaction first, then the lock: the order
        // `write_descriptors` takes them in, or the two could deadlock.
        let txn = crate::errors::ctx(
            self.db.begin_write(),
            "Failed to begin a write to the cache database",
        )?;
        let mut sync = self.descriptor_sync.lock().unwrap();
        if !sync.unsynced {
            // A durable write landed meanwhile; dropping `txn` aborts it.
            return Ok(());
        }
        // An empty commit at the default (immediate) durability persists
        // the non-durable commits before it.
        crate::errors::ctx(txn.commit(), "Failed to persist cache descriptors")?;
        sync.last_durable = Some(Instant::now());
        sync.unsynced = false;
        Ok(())
    }

    /// Store a cache descriptor for a cache key. A transaction, so a reader
    /// never sees a partly written one.
    pub(super) fn store_descriptor(
        &self,
        cache_key: &str,
        descriptor: &CacheDescriptor,
    ) -> Result<()> {
        let data =
            serde_json::to_vec(descriptor).context("Failed to serialize cache descriptor")?;
        self.write_descriptors(|table| {
            table
                .insert(cache_key, data.as_slice())
                .with_context(|| format!("Failed to store cache descriptor {cache_key}"))?;
            Ok(())
        })
    }

    /// The stored bytes of the descriptor under `cache_key`, if there is one.
    fn read_descriptor_bytes(&self, cache_key: &str) -> Result<Option<Vec<u8>>> {
        let txn = crate::errors::ctx(
            self.db.begin_read(),
            "Failed to begin a read of the cache database",
        )?;
        let table = match txn.open_table(DESCRIPTORS_TABLE) {
            Ok(table) => table,
            // Nothing has been cached yet.
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(None),
            Err(e) => {
                return Err(
                    anyhow::Error::from(e).context("Failed to open the cache descriptor table")
                );
            }
        };
        Ok(table
            .get(cache_key)
            .with_context(|| format!("Failed to read cache descriptor {cache_key}"))?
            .map(|value| value.value().to_vec()))
    }

    /// Every stored descriptor, as (key, bytes), for the management commands.
    pub(super) fn descriptor_rows(&self) -> Result<Vec<(String, Vec<u8>)>> {
        use redb::ReadableTable;
        let txn = crate::errors::ctx(
            self.db.begin_read(),
            "Failed to begin a read of the cache database",
        )?;
        let table = match txn.open_table(DESCRIPTORS_TABLE) {
            Ok(table) => table,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
            Err(e) => {
                return Err(
                    anyhow::Error::from(e).context("Failed to open the cache descriptor table")
                );
            }
        };
        let mut rows = Vec::new();
        for row in table
            .iter()
            .context("Failed to iterate the cache descriptors")?
        {
            let (key, value) = row.context("Failed to read a cache descriptor")?;
            rows.push((key.value().to_string(), value.value().to_vec()));
        }
        Ok(rows)
    }

    /// Whether the local store holds a descriptor under `cache_key`.
    pub fn has_descriptor(&self, cache_key: &str) -> bool {
        self.read_descriptor_bytes(cache_key)
            .is_ok_and(|bytes| bytes.is_some())
    }

    /// Read a cache descriptor for a cache key from the local store.
    /// Returns None if not found. Read paths that can legitimately consult
    /// the remote cache should use `get_descriptor_pulling` instead.
    ///
    /// Descriptors are written in transactions, so one that cannot be read
    /// or parsed is damage, not a half-written entry. It still counts as a
    /// miss, so the product rebuilds and rewrites it, but the damage is
    /// reported: `cache trim` refuses to run over such an entry, and a
    /// silent miss here would leave it in place unexplained.
    pub(super) fn get_descriptor(&self, cache_key: &str) -> Option<CacheDescriptor> {
        let data = match self.read_descriptor_bytes(cache_key) {
            Ok(Some(data)) => data,
            Ok(None) => return None,
            Err(e) => {
                crate::output::warn(&format!(
                    "cannot read cache descriptor {cache_key} ({e:#}); treating it as a miss"
                ));
                return None;
            }
        };
        match serde_json::from_slice(&data) {
            Ok(descriptor) => Some(descriptor),
            Err(e) => {
                crate::output::warn(&format!(
                    "cache descriptor {cache_key} is corrupt ({e}); treating it as a miss, the \
                     rebuild rewrites it"
                ));
                None
            }
        }
    }

    /// Read a cache descriptor, falling back to the remote cache on a local
    /// miss when pull is enabled.
    ///
    /// Without this, a populated remote bucket could never satisfy a
    /// restore: every descriptor lookup was local-only, so a machine with a
    /// cold local cache always rebuilt regardless of what the remote held.
    pub(super) fn get_descriptor_pulling(
        &self,
        ctx: &crate::build_context::BuildContext,
        cache_key: &str,
    ) -> Option<CacheDescriptor> {
        if let Some(descriptor) = self.get_descriptor(cache_key) {
            return Some(descriptor);
        }
        if !self.remote_pull {
            return None;
        }
        // A remote miss degrades to a rebuild, which is always correct; only
        // a malformed descriptor is worth reporting, since it means the
        // bucket itself is bad.
        match self.try_fetch_descriptor_from_remote(ctx, cache_key) {
            Ok(descriptor) => descriptor,
            Err(e) => {
                crate::output::warn(&format!(
                    "failed to fetch descriptor from remote cache: {e}"
                ));
                None
            }
        }
    }

    /// Read a cache descriptor for inspection: the local store first, then
    /// the remote cache when pull is enabled — without writing the pulled
    /// descriptor to the local store.
    ///
    /// `get_descriptor_pulling` persists what it pulls, which is right for
    /// a build (the restore that follows needs it) and wrong for the
    /// read-only commands (`status`, `product show`, `build --dry-run`,
    /// `--explain`): inspecting the cache must not populate the local
    /// descriptor table. The remote is still consulted, so the
    /// explanation "would restore from the remote cache" stays truthful —
    /// the same read-without-side-effect that `object_available` does for
    /// objects.
    pub(super) fn get_descriptor_peeking(
        &self,
        ctx: &crate::build_context::BuildContext,
        cache_key: &str,
    ) -> Option<CacheDescriptor> {
        if let Some(descriptor) = self.get_descriptor(cache_key) {
            return Some(descriptor);
        }
        if !self.remote_pull {
            return None;
        }
        match self.peek_descriptor_from_remote(ctx, cache_key) {
            Ok(descriptor) => descriptor,
            Err(e) => {
                crate::output::warn(&format!("failed to read descriptor from remote cache: {e}"));
                None
            }
        }
    }

    /// Return the list of file paths recorded in the product's last tree descriptor.
    pub fn previous_tree_paths(&self, cache_key: &str) -> Vec<PathBuf> {
        match self.get_descriptor(cache_key) {
            Some(CacheDescriptor::Tree { entries }) => {
                entries
                    .into_iter()
                    .filter_map(|e| match super::safe_entry_path(&e.path) {
                        Ok(_) => Some(PathBuf::from(e.path)),
                        // These paths feed fs::remove_file — never delete
                        // outside the project root on a descriptor's say-so.
                        Err(err) => {
                            crate::output::warn(&format!("Ignoring unsafe tree entry: {err}"));
                            None
                        }
                    })
                    .collect()
            }
            _ => Vec::new(),
        }
    }

    /// Store a descriptor locally and, when remote push is on, publish it so
    /// another machine's pull can find it. Objects are pushed by their own
    /// call sites (they must exist before the descriptor that names them),
    /// which pass `objects_pushed`: whether every object the descriptor
    /// names reached the remote. When one did not, the descriptor stays
    /// local: published, it would be a remote entry no machine can restore,
    /// found on every pull until the same key is rebuilt.
    fn store_and_push_descriptor(
        &self,
        ctx: &crate::build_context::BuildContext,
        cache_key: &str,
        descriptor: &CacheDescriptor,
        objects_pushed: bool,
    ) -> Result<()> {
        self.store_descriptor(cache_key, descriptor)?;
        // `remote_push` defaults to true whether or not a remote is
        // configured; with none there is nothing to push and nothing to
        // warn about.
        if self.remote.is_none() {
            return Ok(());
        }
        if self.remote_push && !objects_pushed {
            crate::output::warn(&format!(
                "cache entry {cache_key} not pushed to the remote cache: an object it names could not be pushed"
            ));
        } else if self.remote_push {
            let data = serde_json::to_vec(descriptor)
                .context("Failed to serialize cache descriptor for remote push")?;
            self.try_push_descriptor_to_remote(ctx, cache_key, &data)?;
        }
        Ok(())
    }

    /// Store a marker descriptor (checker passed), publishing it to the
    /// remote cache when push is enabled — the same contract as
    /// `store_blob_descriptor` and `store_tree_descriptor`.
    pub fn store_marker(
        &self,
        ctx: &crate::build_context::BuildContext,
        cache_key: &str,
    ) -> Result<()> {
        self.store_and_push_descriptor(ctx, cache_key, &CacheDescriptor::Marker, true)
    }

    /// Store a blob descriptor (generator produced a single output).
    pub fn store_blob_descriptor(
        &self,
        ctx: &crate::build_context::BuildContext,
        cache_key: &str,
        output_path: &Path,
    ) -> Result<bool> {
        let content = fs::read(output_path)
            .with_context(|| format!("Failed to read output: {}", output_path.display()))?;
        let checksum = self.store_object(&content)?;
        let mode = fs::metadata(output_path)
            .ok()
            .map(|m| crate::platform::get_mode(&m));

        let changed = match self.get_descriptor(cache_key) {
            Some(CacheDescriptor::Blob { checksum: prev, .. }) => prev != checksum,
            _ => true,
        };

        let pushed = self.remote_push && self.try_push_object_to_remote(ctx, &checksum);

        self.store_and_push_descriptor(
            ctx,
            cache_key,
            &CacheDescriptor::Blob { checksum, mode },
            pushed,
        )?;

        Ok(changed)
    }

    /// Store a tree descriptor (a product with several outputs or `output_dirs`).
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
        let mut pushed = true;

        for dir in output_dirs {
            let dir: &Path = dir;
            anyhow::ensure!(
                dir.exists() && dir.is_dir(),
                "Expected output directory not produced: {}",
                dir.display()
            );
            for item in walk_tree(dir)? {
                let file_path = match item {
                    TreeItem::File(path) => path,
                    TreeItem::Link { path, target } => {
                        if is_foreign(&path) {
                            continue;
                        }
                        let target = target.to_str().with_context(|| {
                            format!("Symlink {} has a non-UTF-8 target", path.display())
                        })?;
                        entries.push(TreeEntry {
                            path: path.display().to_string(),
                            checksum: String::new(),
                            mode: None,
                            link: Some(target.to_string()),
                        });
                        continue;
                    }
                };
                if is_foreign(&file_path) {
                    continue;
                }
                let content = fs::read(&file_path)
                    .with_context(|| format!("Failed to read: {}", file_path.display()))?;
                let checksum = self.store_object(&content)?;
                let mode = fs::metadata(&file_path)
                    .ok()
                    .map(|m| crate::platform::get_mode(&m));
                if self.remote_push {
                    pushed &= self.try_push_object_to_remote(ctx, &checksum);
                }
                entries.push(TreeEntry {
                    path: file_path.display().to_string(),
                    checksum,
                    mode,
                    link: None,
                });
            }
        }

        for file_path in output_files {
            anyhow::ensure!(
                file_path.exists(),
                "Expected output file not produced: {}",
                file_path.display()
            );
            let content = fs::read(file_path)
                .with_context(|| format!("Failed to read: {}", file_path.display()))?;
            let checksum = self.store_object(&content)?;
            let mode = fs::metadata(file_path)
                .ok()
                .map(|m| crate::platform::get_mode(&m));
            if self.remote_push {
                pushed &= self.try_push_object_to_remote(ctx, &checksum);
            }
            entries.push(TreeEntry {
                path: Self::path_string(file_path),
                checksum,
                mode,
                link: None,
            });
        }

        // Canonical order: walk_files yields filesystem-dependent read_dir
        // order, so an order shift between runs must not read as a change.
        entries.sort_by(|a, b| a.path.cmp(&b.path));

        let changed = match prev {
            Some(CacheDescriptor::Tree {
                entries: ref prev_entries,
            }) => {
                entries.len() != prev_entries.len()
                    || entries.iter().zip(prev_entries.iter()).any(|(a, b)| {
                        a.checksum != b.checksum || a.path != b.path || a.link != b.link
                    })
            }
            _ => true,
        };

        self.store_and_push_descriptor(ctx, cache_key, &CacheDescriptor::Tree { entries }, pushed)?;
        Ok(changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build_context::BuildContext;

    /// The first write is durable, writes right after it are not, and
    /// `persist` makes them durable; a store that wrote nothing has nothing
    /// to persist.
    #[test]
    fn descriptor_writes_defer_durability_until_persist() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = ObjectStore::new_in(tmp.path());
        let ctx = BuildContext::new();
        let unsynced = |store: &ObjectStore| store.descriptor_sync.lock().unwrap().unsynced;

        store.persist().unwrap();
        assert!(!unsynced(&store), "nothing written, nothing to persist");
        store.store_marker(&ctx, "aa01").unwrap();
        assert!(!unsynced(&store), "the first write of a run is durable");
        store.store_marker(&ctx, "aa02").unwrap();
        assert!(unsynced(&store), "a write right after it is deferred");
        store.persist().unwrap();
        assert!(!unsynced(&store));
        assert!(store.has_descriptor("aa01") && store.has_descriptor("aa02"));
    }

    /// Deferred writes survive closing the database: dropping the store
    /// persists them.
    #[test]
    fn dropping_the_store_persists_deferred_writes() {
        let tmp = tempfile::TempDir::new().unwrap();
        let ctx = BuildContext::new();
        {
            let store = ObjectStore::new_in(tmp.path());
            store.store_marker(&ctx, "bb01").unwrap();
            store.store_marker(&ctx, "bb02").unwrap();
        }
        let store = ObjectStore::new_in(tmp.path());
        assert!(store.has_descriptor("bb01"));
        assert!(store.has_descriptor("bb02"));
    }

    /// The entries are sorted before comparison precisely so that
    /// filesystem-dependent `read_dir` order can't read as a change; content
    /// changes still must.
    #[test]
    fn tree_change_detection_ignores_order_but_sees_content() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = ObjectStore::new_in(tmp.path());
        let ctx = BuildContext::new();
        let key = "abba7777";

        let outdir = tmp.path().join("out");
        fs::create_dir_all(&outdir).unwrap();
        fs::write(outdir.join("a.txt"), b"one").unwrap();
        fs::write(outdir.join("b.txt"), b"two").unwrap();
        let dirs = [std::sync::Arc::new(outdir.clone())];

        assert!(
            store
                .store_tree_descriptor(&ctx, key, &dirs, &[], &|_| false)
                .unwrap(),
            "first store is always a change"
        );
        assert!(
            !store
                .store_tree_descriptor(&ctx, key, &dirs, &[], &|_| false)
                .unwrap(),
            "identical re-store must not read as a change"
        );

        fs::write(outdir.join("a.txt"), b"changed").unwrap();
        assert!(
            store
                .store_tree_descriptor(&ctx, key, &dirs, &[], &|_| false)
                .unwrap(),
            "content change must be detected"
        );
    }

    /// A second store over the same key replaces the first.
    #[test]
    fn descriptor_overwrite_replaces_the_previous_one() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = ObjectStore::new_in(tmp.path());
        let ctx = BuildContext::new();

        store.store_marker(&ctx, "cdcd1212").unwrap();
        store.store_marker(&ctx, "cdcd1212").unwrap();
        assert!(matches!(
            store.get_descriptor("cdcd1212"),
            Some(CacheDescriptor::Marker)
        ));
    }

    /// A missing descriptor and a corrupt one both read as a miss (the
    /// corrupt one also warns), so the product rebuilds and rewrites it.
    #[test]
    fn missing_and_corrupt_descriptors_are_misses() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = ObjectStore::new_in(tmp.path());
        assert!(store.get_descriptor("efef3434").is_none());

        store
            .write_descriptors(|table| {
                table.insert("efef3434", b"{ not json".as_slice())?;
                Ok(())
            })
            .unwrap();
        assert!(store.get_descriptor("efef3434").is_none());
    }
}
