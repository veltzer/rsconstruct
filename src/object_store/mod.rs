mod checksums;
mod config_diff;
mod management;
mod operations;
mod validity;

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use redb::{Database, ReadableDatabase, TableDefinition};

use crate::checksum;
use crate::config::RestoreMethod;
use crate::remote_cache::RemoteCache;

/// Number of hex chars used as the subdirectory prefix for object storage (git-style sharding).
const CHECKSUM_PREFIX_LEN: usize = 2;

/// Iteratively collect all files under a directory.
fn walk_files(dir: &Path) -> Vec<PathBuf> {
    let mut result = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        if let Ok(entries) = fs::read_dir(&current) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.is_file() {
                    result.push(path);
                }
            }
        }
    }
    result
}

const RSB_DIR: &str = ".rsb";
const OBJECTS_DIR: &str = "objects";
const DB_FILE: &str = "db.redb";

const CACHE_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("cache");
const CONFIGS_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("processor_configs");
const MTIME_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("mtime_cache");

/// Cached mtime-to-checksum mapping for a single file
#[derive(Serialize, Deserialize)]
struct MtimeEntry {
    mtime_secs: i64,
    mtime_nanos: u32,
    checksum: String,
}

/// Reason why a product needs to be rebuilt.
#[derive(Debug)]
pub enum RebuildReason {
    /// No cache entry exists for this product
    NoCacheEntry,
    /// Input files have changed since last build
    InputsChanged,
    /// An output file is missing (and can't be restored from cache)
    OutputMissing(String),
    /// Build was forced with --force flag
    Force,
}

impl std::fmt::Display for RebuildReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RebuildReason::NoCacheEntry => write!(f, "no cache entry"),
            RebuildReason::InputsChanged => write!(f, "inputs changed"),
            RebuildReason::OutputMissing(path) => write!(f, "output missing: {}", path),
            RebuildReason::Force => write!(f, "forced"),
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
            ExplainAction::Skip => write!(f, "SKIP (inputs unchanged)"),
            ExplainAction::Restore(reason) => write!(f, "RESTORE ({})", reason),
            ExplainAction::Rebuild(reason) => write!(f, "BUILD ({})", reason),
        }
    }
}

/// Object store for caching build outputs
/// Uses git-like object storage: .rsb/objects/[2 chars]/[rest of hash]
/// Index is stored in a redb embedded key/value database at .rsb/db.redb
pub struct ObjectStore {
    /// Path to .rsb directory
    rsb_dir: PathBuf,
    /// Path to objects directory
    objects_dir: PathBuf,
    /// redb database for cache index
    db: Database,
    /// Method to restore files from cache
    restore_method: RestoreMethod,
    /// Whether to compress cached objects with zstd
    compression: bool,
    /// Optional remote cache backend
    remote: Option<Box<dyn RemoteCache>>,
    /// Whether to push to remote cache
    remote_push: bool,
    /// Whether to pull from remote cache
    remote_pull: bool,
    /// Whether to use mtime pre-check to skip unchanged file checksums
    mtime_check: bool,
}

/// Information about a cached product
#[derive(Debug, Serialize, Deserialize, Clone)]
struct CacheEntry {
    /// Combined checksum of all inputs at time of caching
    input_checksum: String,
    /// List of output files and their checksums
    outputs: Vec<OutputEntry>,
}

/// Information about a single cached output file
#[derive(Debug, Serialize, Deserialize, Clone)]
struct OutputEntry {
    /// Original path of the output file (relative to project root)
    path: String,
    /// Checksum of the output content (used as object store key)
    checksum: String,
    /// Unix file permissions (e.g., 0o755). Used for directory cache restore.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mode: Option<u32>,
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
    pub input_checksum: String,
    /// Output paths and whether the object exists in the store
    pub outputs: Vec<CacheListOutput>,
}

/// Information about a single output in a cache list entry
#[derive(Debug, Serialize)]
pub struct CacheListOutput {
    pub path: String,
    pub exists: bool,
}

/// Options for configuring an ObjectStore instance.
pub struct ObjectStoreOptions {
    pub restore_method: RestoreMethod,
    pub compression: bool,
    pub remote: Option<Box<dyn RemoteCache>>,
    pub remote_push: bool,
    pub remote_pull: bool,
    pub mtime_check: bool,
}

impl ObjectStore {
    pub fn new(opts: ObjectStoreOptions) -> Result<Self> {
        let rsb_dir = PathBuf::from(RSB_DIR);
        let objects_dir = rsb_dir.join(OBJECTS_DIR);
        let db_path = rsb_dir.join(DB_FILE);

        // Ensure .rsb directory exists
        fs::create_dir_all(&rsb_dir)
            .context("Failed to create .rsb directory")?;

        let db = crate::db::open_or_recreate(&db_path, "Cache database")?;

        Ok(Self {
            rsb_dir,
            objects_dir,
            db,
            restore_method: opts.restore_method,
            compression: opts.compression,
            remote: opts.remote,
            remote_push: opts.remote_push,
            remote_pull: opts.remote_pull,
            mtime_check: opts.mtime_check,
        })
    }

    /// Set whether mtime pre-check is enabled.
    pub fn set_mtime_check(&mut self, enabled: bool) {
        self.mtime_check = enabled;
    }

    /// Get a cache entry from the database
    fn get_entry(&self, cache_key: &str) -> Option<CacheEntry> {
        let read_txn = self.db.begin_read().ok()?;
        let table = read_txn.open_table(CACHE_TABLE).ok()?;
        let data = table.get(cache_key).ok()??;
        serde_json::from_slice(data.value()).ok()
    }

    /// Insert a cache entry into the database
    fn insert_entry(&self, cache_key: &str, entry: &CacheEntry) -> Result<()> {
        let value = serde_json::to_vec(entry)
            .context("Failed to serialize cache entry")?;
        let write_txn = self.db.begin_write()
            .context("Failed to begin write transaction")?;
        {
            let mut table = write_txn.open_table(CACHE_TABLE)
                .context("Failed to open cache table")?;
            table.insert(cache_key, value.as_slice())
                .context("Failed to insert cache entry")?;
        }
        write_txn.commit()
            .context("Failed to commit cache entry")?;
        Ok(())
    }

    /// Calculate SHA-256 checksum of a file
    pub fn calculate_checksum(file_path: &Path) -> Result<String> {
        checksum::file_checksum(file_path)
    }

    /// Calculate SHA-256 checksum of bytes
    pub fn calculate_checksum_bytes(data: &[u8]) -> String {
        checksum::bytes_checksum(data)
    }

    /// Get object path for a checksum (e.g., .rsb/objects/ab/cdef123...)
    fn object_path(&self, checksum: &str) -> PathBuf {
        let (prefix, rest) = checksum.split_at(CHECKSUM_PREFIX_LEN.min(checksum.len()));
        self.objects_dir.join(prefix).join(rest)
    }

    /// Store content in object store, returns checksum.
    /// The checksum is always computed on the **original** (uncompressed) content
    /// so cache keys remain stable regardless of compression setting.
    /// Objects are made read-only to prevent accidental modification via hardlinks.
    fn store_object(&self, content: &[u8]) -> Result<String> {
        let checksum = Self::calculate_checksum_bytes(content);
        let object_path = self.object_path(&checksum);

        // Only write if not already stored
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
            // Make read-only to prevent corruption via hardlinks
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
    fn has_object(&self, checksum: &str) -> bool {
        self.object_path(checksum).exists()
    }

    /// Restore a file from the object store using configured method.
    ///
    /// Cache objects are read-only to prevent corruption. For hardlinks, the
    /// restored file shares the same inode and is therefore also read-only —
    /// any tool that tries to write in-place will get a permission error,
    /// which is the desired protection. For copies, we make the output
    /// writable since it's an independent file that can't corrupt the cache.
    ///
    /// When compression is enabled, the stored blob is zstd-compressed so we
    /// must decompress before writing the output file. (Hardlink path is
    /// unreachable due to config validation.)
    fn restore_file(&self, checksum: &str, output_path: &Path) -> Result<()> {
        let object_path = self.object_path(checksum);

        if self.compression {
            // Decompress and write the output file
            let content = self.read_object(checksum)?;
            fs::write(output_path, &content)
                .with_context(|| format!("Failed to write decompressed output: {}", output_path.display()))?;
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o644);
            fs::set_permissions(output_path, perms)
                .context("Failed to make restored file writable")?;
            return Ok(());
        }

        match self.restore_method {
            RestoreMethod::Hardlink => {
                fs::hard_link(&object_path, output_path)
                    .with_context(|| format!("Failed to hard link from cache: {}. If on a cross-filesystem setup, set restore_method = \"copy\" in rsb.toml.", checksum))?;
            }
            RestoreMethod::Copy => {
                fs::copy(&object_path, output_path)
                    .with_context(|| format!("Failed to copy from cache: {}", checksum))?;
                // Make the copy writable (owner rw) — it's independent from the cache object
                use std::os::unix::fs::PermissionsExt;
                let perms = std::fs::Permissions::from_mode(0o644);
                fs::set_permissions(output_path, perms)
                    .context("Failed to make restored file writable")?;
            }
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
    fn path_string(path: &Path) -> String {
        path.display().to_string()
    }
}
