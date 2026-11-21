pub mod blobs;
mod config_diff;
mod descriptors;
mod management;
mod operations;
mod owners;
mod restore;

use anyhow::{Context, Result};
use redb::{Database, TableDefinition};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::RestoreMethod;
use crate::remote_cache::RemoteCache;

/// Number of hex chars used as the subdirectory prefix for object storage (git-style sharding).
pub const CHECKSUM_PREFIX_LEN: usize = 2;

/// Iteratively collect all files under a directory.
///
/// A directory that cannot be read is an error, not an empty one: the
/// callers decide from this list what a cached tree holds and which objects
/// nothing references, so a silently skipped subdirectory would cache a
/// tree with files missing, or collect live objects as garbage. Only a
/// directory that does not exist (the root, or one removed while walking)
/// contributes nothing.
pub fn walk_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut result = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let entries = match fs::read_dir(&current) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                return Err(e)
                    .with_context(|| format!("Failed to read directory {}", current.display()));
            }
        };
        for entry in entries {
            let entry = entry.with_context(|| {
                format!("Failed to read an entry of directory {}", current.display())
            })?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.is_file() {
                // Temp files (temp-then-rename in flight, or orphaned by
                // a crash) are included deliberately: `trim` collects
                // them as unreferenced garbage — a filter here once made
                // crash-orphaned temp files permanently unreclaimable by
                // every management command. redb's exclusive lock means
                // no second process is mid-write while we walk.
                result.push(path);
            }
        }
    }
    Ok(result)
}

/// Something a tool left under an output directory.
pub enum TreeItem {
    File(PathBuf),
    /// A symbolic link and what it points at, exactly as the tool wrote it.
    Link {
        path: PathBuf,
        target: PathBuf,
    },
}

/// Collect what is under an output directory without following symbolic
/// links: a link is an item of its own, whatever it points at.
///
/// `walk_files` follows them, which is right for the store's own
/// directories (they hold none) and wrong for a tool's output: a link to a
/// file was cached as a second regular file, a link to a directory as a
/// copy of that whole directory, and a dangling link was dropped, so a
/// restored tree was not the tree the tool had written.
pub fn walk_tree(dir: &Path) -> Result<Vec<TreeItem>> {
    let mut result = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let entries = match fs::read_dir(&current) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                return Err(e)
                    .with_context(|| format!("Failed to read directory {}", current.display()));
            }
        };
        for entry in entries {
            let entry = entry.with_context(|| {
                format!("Failed to read an entry of directory {}", current.display())
            })?;
            let path = entry.path();
            // `DirEntry::file_type` does not follow a link.
            let file_type = entry
                .file_type()
                .with_context(|| format!("Failed to stat {}", path.display()))?;
            if file_type.is_symlink() {
                let target = fs::read_link(&path)
                    .with_context(|| format!("Failed to read symlink {}", path.display()))?;
                result.push(TreeItem::Link { path, target });
            } else if file_type.is_dir() {
                stack.push(path);
            } else if file_type.is_file() {
                result.push(TreeItem::File(path));
            }
        }
    }
    Ok(result)
}

/// Whether `path` is a symbolic link pointing at exactly `target`.
fn is_link_to(path: &Path, target: &str) -> bool {
    fs::read_link(path).is_ok_and(|current| current == Path::new(target))
}

/// Reject a tree in which one entry lies below another entry that is a
/// symbolic link. A tree recorded here never has one (`walk_tree` does not
/// descend through links); a descriptor from elsewhere that did would have
/// the restore write through the link, to wherever it points.
fn validate_tree_entries(entries: &[TreeEntry]) -> Result<()> {
    let links: std::collections::HashSet<&Path> = entries
        .iter()
        .filter(|e| e.link.is_some())
        .map(|e| Path::new(&e.path))
        .collect();
    for entry in entries {
        if let Some(link) = Path::new(&entry.path)
            .ancestors()
            .skip(1)
            .find(|dir| links.contains(dir))
        {
            anyhow::bail!(
                "cache descriptor entry '{}' lies below the symlink entry '{}'",
                entry.path,
                link.display()
            );
        }
    }
    Ok(())
}

/// Reject descriptor entry paths with `..` components — they could write
/// (restore) or delete (stale-output cleanup via `previous_tree_paths`)
/// outside the project root. Absolute paths are additionally rejected at the
/// remote-fetch boundary (`try_fetch_descriptor_from_remote`): production
/// entries are always project-root-relative, so an absolute path can only
/// come from a foreign descriptor.
fn safe_entry_path(raw: &str) -> Result<&Path> {
    let path = Path::new(raw);
    if path
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        anyhow::bail!("cache descriptor entry path '{raw}' escapes the project root");
    }
    Ok(path)
}

/// A key (an object checksum or a descriptor key) as its shard directory
/// and the file name inside it: the first `CHECKSUM_PREFIX_LEN` characters,
/// then the rest. A key shorter than the prefix is all directory. This is
/// the one place the split is made; the local paths and the remote keys
/// are both built from it.
fn shard(key: &str) -> (&str, &str) {
    key.split_at_checked(CHECKSUM_PREFIX_LEN.min(key.len()))
        .unwrap_or((key, ""))
}

/// The remote cache key of an object: `objects/<shard>/<rest>`.
fn remote_object_key(checksum: &str) -> String {
    let (prefix, rest) = shard(checksum);
    format!("objects/{prefix}/{rest}")
}

/// Whether `text` has the form of an object's name: the 64 lowercase hex
/// digits `checksum::bytes_checksum` produces. Checked on every checksum
/// that arrives from outside (a remote descriptor) before it is turned into
/// a path below the objects directory.
fn is_object_checksum(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

const RSBUILD_DIR: &str = ".rsconstruct";
const OBJECTS_DIR: &str = "objects";
const DB_FILE: &str = "db.redb";

/// Descriptor key → the descriptor, as JSON. See `descriptors.rs`.
const DESCRIPTORS_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("descriptors");
const CONFIGS_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("processor_configs");
/// `Product::owner_key` → the descriptor key whose tree is on disk for that
/// product. See `ObjectStore::record_last_tree`.
const LAST_TREE_TABLE: TableDefinition<&str, &str> = TableDefinition::new("product_last_tree");

/// Reason why a product needs to be rebuilt.
#[derive(Debug)]
pub enum RebuildReason {
    /// No cache entry exists for this product (new or inputs changed)
    NoCacheEntry,
    /// An output file is missing (and can't be restored from cache)
    OutputMissing(String),
    /// Build was forced with --force flag
    Force,
    /// A product this one depends on will be rebuilt or restored first, so
    /// its inputs are about to change (a prediction; see `classify_products`)
    DependencyChanged,
    /// An input could not be read, so no descriptor key exists to look up
    InputUnreadable,
}

impl std::fmt::Display for RebuildReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoCacheEntry => write!(f, "no cache entry"),
            Self::OutputMissing(path) => write!(f, "output missing: {path}"),
            Self::Force => write!(f, "forced"),
            Self::DependencyChanged => write!(f, "a dependency will be rebuilt or restored"),
            Self::InputUnreadable => write!(f, "an input could not be read"),
        }
    }
}

/// The action the executor will take for a product, with an explanation.
#[derive(Debug)]
pub enum ExplainAction {
    /// Product is up-to-date, will be skipped
    Skip,
    /// Product will be restored from cache
    Restore(RebuildReason),
    /// Product will be rebuilt
    Rebuild(RebuildReason),
}

impl std::fmt::Display for ExplainAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Skip => write!(f, "SKIP (inputs unchanged)"),
            Self::Restore(reason) => write!(f, "RESTORE ({reason})"),
            Self::Rebuild(reason) => write!(f, "BUILD ({reason})"),
        }
    }
}

/// Object store for caching build outputs.
/// Uses git-like object storage: .rsconstruct/objects/[2 chars]/[rest of hash]
/// Descriptors and the index are stored in a redb embedded key/value
/// database at .rsconstruct/db.redb
///
/// Methods are split across submodules:
/// - `blobs.rs` — content-addressed blob read/write/restore
/// - `descriptors.rs` — cache descriptor CRUD and `store_marker/blob/tree`
/// - `restore.rs` — `restore_from_descriptor`, `needs_rebuild`, `can_restore`, explain
/// - `management.rs` — size, trim, `remove_stale`, list, stats
/// - `operations.rs` — remote cache push/fetch
/// - `config_diff.rs` — processor config change tracking
pub struct ObjectStore {
    pub(super) objects_dir: PathBuf,
    pub(super) db: Database,
    /// When descriptor writes last reached the disk, and whether any have
    /// been committed since without reaching it. See
    /// `ObjectStore::write_descriptors`.
    pub(super) descriptor_sync: std::sync::Mutex<descriptors::DescriptorSync>,
    pub(super) restore_method: RestoreMethod,
    pub(super) compression: bool,
    pub(super) remote: Option<Box<dyn RemoteCache>>,
    pub(super) remote_push: bool,
    pub(super) remote_pull: bool,
}

/// The end of every command that opened the store: deferred descriptor
/// writes reach the disk (see `descriptors.rs`). A failure here cannot fail
/// the command any more, so it is reported; the cost is products that
/// rebuild next time.
impl Drop for ObjectStore {
    fn drop(&mut self) {
        if let Err(e) = self.persist() {
            crate::output::warn(&format!(
                "cache entries from this run may not have been saved: {e:#}"
            ));
        }
    }
}

/// A cache descriptor stored in the object store at the cache key path.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum CacheDescriptor {
    #[serde(rename = "marker")]
    Marker,
    #[serde(rename = "blob")]
    Blob {
        checksum: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mode: Option<u32>,
    },
    #[serde(rename = "tree")]
    Tree { entries: Vec<TreeEntry> },
}

/// A single file entry in a tree descriptor.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TreeEntry {
    pub(super) path: String,
    /// The object holding the file's content; empty for a symbolic link,
    /// which has no object.
    pub(super) checksum: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) mode: Option<u32>,
    /// The target of a symbolic link, as the tool wrote it. `None` for a
    /// regular file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) link: Option<String>,
}

/// Per-processor cache statistics
#[derive(Debug, Default, Serialize)]
pub struct ProcessorCacheStats {
    pub entry_count: usize,
    pub output_count: usize,
    pub output_bytes: u64,
}

/// Information about a cache entry for display
#[derive(Debug, Serialize)]
pub struct CacheListEntry {
    pub cache_key: String,
    pub outputs: Vec<CacheListOutput>,
}

/// Information about a single output in a cache list entry
#[derive(Debug, Serialize)]
pub struct CacheListOutput {
    pub path: String,
    pub exists: bool,
}

/// Options for configuring an `ObjectStore` instance.
pub struct ObjectStoreOptions {
    pub restore_method: RestoreMethod,
    pub compression: bool,
    pub remote: Option<Box<dyn RemoteCache>>,
    pub remote_push: bool,
    pub remote_pull: bool,
}

impl ObjectStore {
    pub fn new(opts: ObjectStoreOptions) -> Result<Self> {
        let rsconstruct_dir = PathBuf::from(RSBUILD_DIR);
        let objects_dir = rsconstruct_dir.join(OBJECTS_DIR);
        let db_path = rsconstruct_dir.join(DB_FILE);

        fs::create_dir_all(&rsconstruct_dir).context("Failed to create .rsconstruct directory")?;

        let db = crate::db::open(&db_path, "Cache database")?;

        Ok(Self {
            objects_dir,
            db,
            descriptor_sync: std::sync::Mutex::default(),
            restore_method: opts.restore_method,
            compression: opts.compression,
            remote: opts.remote,
            remote_push: opts.remote_push,
            remote_pull: opts.remote_pull,
        })
    }

    /// Test-only store rooted at `dir` with its own db file — production
    /// `new()` hardcodes the CWD-relative `.rsconstruct` and cannot be
    /// pointed at a tempdir. The redb file allows only one open handle, so
    /// tests that build several stores over the same dir must give each its
    /// own `db_name`.
    #[cfg(test)]
    pub(crate) fn new_at(dir: &Path, db_name: &str) -> Self {
        Self {
            objects_dir: dir.join(OBJECTS_DIR),
            db: crate::db::open(&dir.join(db_name), "test cache database").unwrap(),
            descriptor_sync: std::sync::Mutex::default(),
            restore_method: RestoreMethod::Hardlink,
            compression: false,
            remote: None,
            remote_push: false,
            remote_pull: false,
        }
    }

    /// Test-only store rooted at `dir` with the default db file name.
    #[cfg(test)]
    pub(crate) fn new_in(dir: &Path) -> Self {
        Self::new_at(dir, "db.redb")
    }

    /// Test-only store with a file:// remote and push+pull enabled, for
    /// exercising the remote round-trip without an S3 bucket.
    #[cfg(test)]
    pub(crate) fn new_with_remote(dir: &Path, db_name: &str, remote_dir: &Path) -> Self {
        fs::create_dir_all(dir).unwrap();
        let mut store = Self::new_at(dir, db_name);
        store.remote = Some(Box::new(
            crate::remote_cache::FileBackend::new(&format!("file://{}", remote_dir.display()))
                .unwrap(),
        ));
        store.remote_push = true;
        store.remote_pull = true;
        store
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, link: Option<&str>) -> TreeEntry {
        TreeEntry {
            path: path.to_string(),
            checksum: String::new(),
            mode: None,
            link: link.map(str::to_string),
        }
    }

    /// A tree with an entry below one of its own symlink entries would be
    /// restored through the link, to wherever it points. No tree recorded
    /// here has that shape, so one that does is rejected.
    #[test]
    fn an_entry_below_a_symlink_entry_is_rejected() {
        let ok = [
            entry("out/link", Some("/elsewhere")),
            entry("out/linked", None),
            entry("out/sub/file", None),
        ];
        validate_tree_entries(&ok).unwrap();

        let through = [
            entry("out/link", Some("/elsewhere")),
            entry("out/link/sub/file", None),
        ];
        let err = validate_tree_entries(&through).unwrap_err().to_string();
        assert!(err.contains("out/link/sub/file"), "{err}");
    }

    /// The walk reports a link as a link, to a file, to a directory or to
    /// nothing, and does not descend through one.
    #[test]
    fn walk_tree_does_not_follow_links() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("sub")).unwrap();
        fs::write(root.join("sub/file"), "x").unwrap();
        crate::platform::symlink_file(Path::new("sub"), &root.join("dir-link")).unwrap();
        crate::platform::symlink_file(Path::new(".."), &root.join("sub/up")).unwrap();
        crate::platform::symlink_file(Path::new("nowhere"), &root.join("dangling")).unwrap();

        let mut seen: Vec<String> = walk_tree(root)
            .unwrap()
            .into_iter()
            .map(|item| match item {
                TreeItem::File(path) => {
                    format!("file {}", path.strip_prefix(root).unwrap().display())
                }
                TreeItem::Link { path, target } => format!(
                    "link {} -> {}",
                    path.strip_prefix(root).unwrap().display(),
                    target.display()
                ),
            })
            .collect();
        seen.sort();
        assert_eq!(
            seen,
            [
                "file sub/file",
                "link dangling -> nowhere",
                "link dir-link -> sub",
                "link sub/up -> ..",
            ]
        );
    }
}
