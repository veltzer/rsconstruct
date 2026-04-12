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

const RSBUILD_DIR: &str = ".rsconstruct";
const OBJECTS_DIR: &str = "objects";
const DESCRIPTORS_DIR: &str = "descriptors";
const DB_FILE: &str = "db.redb";

const CACHE_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("cache");
const CONFIGS_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("processor_configs");

/// Reason why a product needs to be rebuilt.
#[derive(Debug)]
pub enum RebuildReason {
    /// No cache entry exists for this product (new or inputs changed)
    NoCacheEntry,
    /// An output file is missing (and can't be restored from cache)
    OutputMissing(String),
    /// Build was forced with --force flag
    Force,
}

impl std::fmt::Display for RebuildReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RebuildReason::NoCacheEntry => write!(f, "no cache entry"),
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
/// Uses git-like object storage: .rsconstruct/objects/[2 chars]/[rest of hash]
/// Index is stored in a redb embedded key/value database at .rsconstruct/db.redb
pub struct ObjectStore {
    /// Path to objects directory (content-addressed blobs)
    objects_dir: PathBuf,
    /// Path to descriptors directory (cache descriptors keyed by cache key hash)
    descriptors_dir: PathBuf,
    /// redb database for mtime cache and config tracking
    db: Database,
    /// Method to restore files from cache
    restore_method: RestoreMethod,
    /// Whether to compress cached objects with zstd
    compression: bool,
    /// Optional remote cache backend
    remote: Option<Box<dyn RemoteCache>>,
    /// Whether to push to remote cache
    remote_push: bool,
    /// Whether to pull from remote cache.
    /// Wired into the constructor but not yet consulted by any read path —
    /// remote-pull integration is scaffolded in `operations.rs` (the
    /// `try_fetch_*` helpers) but not yet called from the executor.
    #[allow(dead_code)]
    remote_pull: bool,
}

/// A cache descriptor stored in the object store at the cache key path.
/// This is the top-level object that describes what a product produced.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
enum CacheDescriptor {
    /// Checker: no outputs. Presence means the check passed.
    #[serde(rename = "marker")]
    Marker,
    /// Generator: single output file. Points to a content-addressed blob.
    #[serde(rename = "blob")]
    Blob {
        checksum: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mode: Option<u32>,
    },
    /// Creator/creator: multiple output files.
    #[serde(rename = "tree")]
    Tree {
        entries: Vec<TreeEntry>,
    },
}

/// A single file entry in a tree descriptor.
#[derive(Debug, Serialize, Deserialize, Clone)]
struct TreeEntry {
    /// Path of the output file (relative to project root)
    path: String,
    /// Checksum of the file content (blob key in object store)
    checksum: String,
    /// Unix file permissions
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mode: Option<u32>,
}

// --- Legacy types kept temporarily for migration ---

/// Information about a cached product (legacy DB format)
#[derive(Debug, Serialize, Deserialize, Clone)]
struct CacheEntry {
    input_checksum: String,
    outputs: Vec<OutputEntry>,
}

/// Information about a single cached output file (legacy DB format)
#[derive(Debug, Serialize, Deserialize, Clone)]
struct OutputEntry {
    path: String,
    checksum: String,
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
}

impl ObjectStore {
    pub fn new(opts: ObjectStoreOptions) -> Result<Self> {
        let rsconstruct_dir = PathBuf::from(RSBUILD_DIR);
        let objects_dir = rsconstruct_dir.join(OBJECTS_DIR);
        let db_path = rsconstruct_dir.join(DB_FILE);

        // Ensure .rsconstruct directory exists
        fs::create_dir_all(&rsconstruct_dir)
            .context("Failed to create .rsconstruct directory")?;

        let db = crate::db::open_or_recreate(&db_path, "Cache database")?;

        let descriptors_dir = rsconstruct_dir.join(DESCRIPTORS_DIR);

        Ok(Self {
            objects_dir,
            descriptors_dir,
            db,
            restore_method: opts.restore_method,
            compression: opts.compression,
            remote: opts.remote,
            remote_push: opts.remote_push,
            remote_pull: opts.remote_pull,
        })
    }

    /// Check if a cache entry exists for the given key (i.e. the product has been built before).
    pub fn has_cache_entry(&self, cache_key: &str) -> bool {
        self.get_entry(cache_key).is_some()
    }

    /// Get a cache entry from the database
    fn get_entry(&self, cache_key: &str) -> Option<CacheEntry> {
        let read_txn = self.db.begin_read().ok()?;
        let table = read_txn.open_table(CACHE_TABLE).ok()?;
        let data = table.get(cache_key).ok()??;
        serde_json::from_slice(data.value()).ok()
    }



    // --- Descriptor-based cache (new system) ---

    /// Path for a cache descriptor, sharded like objects.
    fn descriptor_path(&self, descriptor_key: &str) -> PathBuf {
        let (prefix, rest) = descriptor_key.split_at(CHECKSUM_PREFIX_LEN.min(descriptor_key.len()));
        self.descriptors_dir.join(prefix).join(rest)
    }

    /// Store a cache descriptor for a cache key.
    fn store_descriptor(&self, cache_key: &str, descriptor: &CacheDescriptor) -> Result<()> {
        let path = self.descriptor_path(cache_key);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .context("Failed to create descriptor directory")?;
        }
        let data = serde_json::to_vec(descriptor)
            .context("Failed to serialize cache descriptor")?;
        // Remove existing file if read-only (from a previous build)
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
    fn get_descriptor(&self, cache_key: &str) -> Option<CacheDescriptor> {
        let path = self.descriptor_path(cache_key);
        let data = fs::read(&path).ok()?;
        serde_json::from_slice(&data).ok()
    }

    /// Return the list of file paths recorded in the product's last tree descriptor,
    /// or an empty vec if there is no prior tree (first build, or marker/blob descriptor).
    /// Used to clean ONLY the files a Creator owned in its previous run, rather than
    /// wiping entire output_dirs (which would destroy other processors' contributions).
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
    pub fn store_blob_descriptor(&self, cache_key: &str, output_path: &Path) -> Result<bool> {
        let content = fs::read(output_path)
            .with_context(|| format!("Failed to read output: {}", output_path.display()))?;
        let checksum = self.store_object(&content)?;
        let mode = fs::metadata(output_path).ok()
            .and_then(|m| crate::platform::get_mode(&m));

        // Check if changed vs previous descriptor
        let changed = match self.get_descriptor(cache_key) {
            Some(CacheDescriptor::Blob { checksum: prev, .. }) => prev != checksum,
            _ => true,
        };

        if self.remote_push {
            self.try_push_object_to_remote(&checksum)?;
        }

        self.store_descriptor(cache_key, &CacheDescriptor::Blob {
            checksum,
            mode,
        })?;

        Ok(changed)
    }

    /// Store a tree descriptor (creator/creator produced multiple outputs).
    /// Walks all output_dirs and collects output_files.
    ///
    /// `is_foreign`: predicate that returns true for paths declared as outputs of OTHER
    /// products. These files live in a shared output directory but are owned by a
    /// different processor; they are skipped so that restore never clobbers them.
    pub fn store_tree_descriptor(
        &self,
        cache_key: &str,
        output_dirs: &[std::sync::Arc<PathBuf>],
        output_files: &[PathBuf],
        is_foreign: &dyn Fn(&Path) -> bool,
    ) -> Result<bool> {
        let prev = self.get_descriptor(cache_key);
        let mut entries = Vec::new();

        // Walk output directories
        for dir in output_dirs {
            let dir: &Path = dir;
            anyhow::ensure!(dir.exists() && dir.is_dir(),
                "Expected output directory not produced: {}", dir.display());
            for file_path in walk_files(dir) {
                // Skip paths owned by another processor that shares this directory.
                if is_foreign(&file_path) {
                    continue;
                }
                let content = fs::read(&file_path)
                    .with_context(|| format!("Failed to read: {}", file_path.display()))?;
                let checksum = self.store_object(&content)?;
                let mode = fs::metadata(&file_path).ok()
                    .and_then(|m| crate::platform::get_mode(&m));
                if self.remote_push {
                    self.try_push_object_to_remote(&checksum)?;
                }
                entries.push(TreeEntry {
                    path: file_path.display().to_string(),
                    checksum,
                    mode,
                });
            }
        }

        // Individual output files
        for file_path in output_files {
            anyhow::ensure!(file_path.exists(),
                "Expected output file not produced: {}", file_path.display());
            let content = fs::read(file_path)
                .with_context(|| format!("Failed to read: {}", file_path.display()))?;
            let checksum = self.store_object(&content)?;
            let mode = fs::metadata(file_path).ok()
                .and_then(|m| crate::platform::get_mode(&m));
            if self.remote_push {
                self.try_push_object_to_remote(&checksum)?;
            }
            entries.push(TreeEntry {
                path: Self::path_string(file_path),
                checksum,
                mode,
            });
        }

        // Detect changes
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

    /// Restore outputs from a cache descriptor. Returns Ok(true) if restored.
    /// For blob descriptors, `output_paths` provides the target path (the descriptor
    /// does not store it — the product knows where its output goes).
    pub fn restore_from_descriptor(&self, cache_key: &str, output_paths: &[PathBuf]) -> Result<bool> {
        let descriptor = match self.get_descriptor(cache_key) {
            Some(d) => d,
            None => return Ok(false),
        };
        match descriptor {
            CacheDescriptor::Marker => Ok(true),
            CacheDescriptor::Blob { checksum, mode } => {
                let output_path = match output_paths.first() {
                    Some(p) => p,
                    None => return Ok(true), // no output to restore
                };
                if output_path.exists() {
                    return Ok(true);
                }
                if !self.has_object(&checksum) {
                    return Ok(false);
                }
                if let Some(parent) = output_path.parent() {
                    fs::create_dir_all(parent)
                        .with_context(|| format!("Failed to create output directory: {}", parent.display()))?;
                }
                self.restore_file(&checksum, output_path)
                    .with_context(|| format!("Failed to restore blob to: {}", output_path.display()))?;
                if let Some(m) = mode {
                    crate::platform::set_permissions_mode(output_path, m)
                        .with_context(|| format!("Failed to set permissions on: {}", output_path.display()))?;
                }
                Ok(true)
            }
            CacheDescriptor::Tree { entries } => {
                for entry in &entries {
                    let file_path = Path::new(&entry.path);
                    // Skip files that exist with the correct checksum
                    if file_path.exists() {
                        if let Ok(existing) = Self::calculate_checksum(file_path) {
                            if existing == entry.checksum {
                                continue;
                            }
                        }
                        // Wrong checksum — remove and re-restore
                        fs::remove_file(file_path)
                            .with_context(|| format!("Failed to remove stale cached file: {}", file_path.display()))?;
                    }
                    if !self.has_object(&entry.checksum) {
                        return Ok(false);
                    }
                    if let Some(parent) = file_path.parent() {
                        fs::create_dir_all(parent)
                            .with_context(|| format!("Failed to create directory for tree restore: {}", parent.display()))?;
                    }
                    self.restore_file(&entry.checksum, file_path)
                        .with_context(|| format!("Failed to restore tree entry: {}", file_path.display()))?;
                    if let Some(m) = entry.mode {
                        crate::platform::set_permissions_mode(file_path, m)
                            .with_context(|| format!("Failed to set permissions on: {}", file_path.display()))?;
                    }
                }
                Ok(true)
            }
        }
    }

    /// Check if a product needs rebuilding based on its descriptor.
    /// Returns true if no descriptor exists or any output is missing.
    /// For blob descriptors, `output_paths` provides the paths to check.
    pub fn needs_rebuild_descriptor(&self, cache_key: &str, output_paths: &[PathBuf]) -> bool {
        let descriptor = match self.get_descriptor(cache_key) {
            Some(d) => d,
            None => return true,
        };
        match descriptor {
            CacheDescriptor::Marker => false,
            CacheDescriptor::Blob { .. } => {
                output_paths.iter().any(|p| !p.exists())
            }
            CacheDescriptor::Tree { entries } => {
                entries.iter().any(|e| {
                    let p = Path::new(&e.path);
                    !p.exists() || Self::calculate_checksum(p).ok().as_ref() != Some(&e.checksum)
                })
            }
        }
    }

    /// Check if outputs can be restored from a descriptor.
    pub fn can_restore_descriptor(&self, cache_key: &str) -> bool {
        let descriptor = match self.get_descriptor(cache_key) {
            Some(d) => d,
            None => return false,
        };
        match descriptor {
            CacheDescriptor::Marker => true,
            CacheDescriptor::Blob { checksum, .. } => self.has_object(&checksum),
            CacheDescriptor::Tree { entries } => {
                entries.iter().all(|e| self.has_object(&e.checksum))
            }
        }
    }

    /// Explain what action will be taken based on descriptor state.
    /// For blob descriptors, `output_paths` provides the paths to check.
    pub fn explain_descriptor(&self, descriptor_key: &str, output_paths: &[PathBuf], force: bool) -> ExplainAction {
        if force {
            return ExplainAction::Rebuild(RebuildReason::Force);
        }
        let descriptor = match self.get_descriptor(descriptor_key) {
            Some(d) => d,
            None => return ExplainAction::Rebuild(RebuildReason::NoCacheEntry),
        };
        match descriptor {
            CacheDescriptor::Marker => ExplainAction::Skip,
            CacheDescriptor::Blob { checksum, .. } => {
                for p in output_paths {
                    if !p.exists() {
                        let display = p.display().to_string();
                        if self.has_object(&checksum) {
                            return ExplainAction::Restore(RebuildReason::OutputMissing(display));
                        } else {
                            return ExplainAction::Rebuild(RebuildReason::OutputMissing(display));
                        }
                    }
                }
                ExplainAction::Skip
            }
            CacheDescriptor::Tree { entries } => {
                for entry in &entries {
                    let p = Path::new(&entry.path);
                    let needs_restore = !p.exists()
                        || Self::calculate_checksum(p).ok().as_ref() != Some(&entry.checksum);
                    if needs_restore {
                        if self.has_object(&entry.checksum) {
                            return ExplainAction::Restore(RebuildReason::OutputMissing(entry.path.clone()));
                        } else {
                            return ExplainAction::Rebuild(RebuildReason::OutputMissing(entry.path.clone()));
                        }
                    }
                }
                ExplainAction::Skip
            }
        }
    }

    // --- End descriptor-based cache ---

    /// Calculate SHA-256 checksum of a file
    pub fn calculate_checksum(file_path: &Path) -> Result<String> {
        checksum::file_checksum(file_path)
    }

    /// Calculate SHA-256 checksum of bytes
    pub fn calculate_checksum_bytes(data: &[u8]) -> String {
        checksum::bytes_checksum(data)
    }

    /// Get object path for a checksum (e.g., .rsconstruct/objects/ab/cdef123...)
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
                // Make the copy writable (owner rw) — it's independent from the cache object
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
    fn path_string(path: &Path) -> String {
        path.display().to_string()
    }
}
