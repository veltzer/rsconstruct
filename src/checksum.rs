use anyhow::{Context, Result};
use redb::{Database, ReadableDatabase, TableDefinition};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

const MTIME_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("mtime_cache");

/// Cached mtime-to-checksum mapping for a single file
#[derive(Serialize, Deserialize)]
struct MtimeEntry {
    mtime_secs: i64,
    mtime_nanos: u32,
    checksum: String,
}

/// Global in-memory checksum cache. Avoids re-reading and re-hashing
/// the same file multiple times within a single build run.
static CACHE: Mutex<Option<HashMap<PathBuf, String>>> = Mutex::new(None);

/// Global mtime database, opened lazily on first use.
static MTIME_DB: Mutex<Option<Database>> = Mutex::new(None);

/// Whether mtime pre-check is enabled (set via `set_mtime_check`).
static MTIME_ENABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Set whether mtime pre-check is enabled.
pub(crate) fn set_mtime_check(enabled: bool) {
    MTIME_ENABLED.store(enabled, std::sync::atomic::Ordering::Relaxed);
}

/// Open or get the mtime database.
fn get_mtime_db() -> Result<std::sync::MutexGuard<'static, Option<Database>>> {
    let mut guard = MTIME_DB.lock().unwrap();
    if guard.is_none() {
        let dir = PathBuf::from(".rsconstruct");
        ctx!(fs::create_dir_all(&dir), "Failed to create .rsconstruct directory")?;
        let db = crate::db::open_or_recreate(&dir.join("mtime.redb"), "Mtime cache")?;
        *guard = Some(db);
    }
    Ok(guard)
}

/// Calculate SHA-256 checksum of a file's contents, using the global cache.
/// First call for a given path reads the file and caches the result.
/// Subsequent calls return the cached value.
pub(crate) fn file_checksum(path: &Path) -> Result<String> {
    let mut guard = CACHE.lock().unwrap();
    let cache = guard.get_or_insert_with(HashMap::new);
    if let Some(cached) = cache.get(path) {
        return Ok(cached.clone());
    }
    let contents = fs::read(path)
        .with_context(|| format!("Failed to read file for checksum: {}", path.display()))?;
    let checksum = hex::encode(Sha256::digest(&contents));
    cache.insert(path.to_path_buf(), checksum.clone());
    Ok(checksum)
}

/// Get checksum using mtime pre-check to avoid re-reading unchanged files.
/// Returns the checksum and optionally a dirty mtime entry to flush.
fn fast_checksum(path: &Path) -> Result<(String, Option<(String, MtimeEntry)>)> {
    let metadata = fs::metadata(path)
        .with_context(|| format!("Failed to stat file: {}", path.display()))?;
    let mtime = metadata.modified()
        .with_context(|| format!("Failed to get mtime: {}", path.display()))?;
    let duration = mtime.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
    let mtime_secs = i64::try_from(duration.as_secs()).unwrap_or(i64::MAX);
    let mtime_nanos = duration.subsec_nanos();

    let path_str = path.display().to_string();

    // Check mtime cache in DB
    let db_guard = get_mtime_db()?;
    let cached = if let Some(ref db) = *db_guard {
        let read_txn = db.begin_read().context("Failed to begin read transaction for mtime cache")?;
        match read_txn.open_table(MTIME_TABLE) {
            Ok(table) => {
                table.get(path_str.as_str()).ok()
                    .flatten()
                    .and_then(|data| serde_json::from_slice::<MtimeEntry>(data.value()).ok())
            }
            Err(_) => None,
        }
    } else {
        None
    };
    drop(db_guard);

    if let Some(ref entry) = cached {
        if entry.mtime_secs == mtime_secs && entry.mtime_nanos == mtime_nanos {
            // Also populate the in-memory cache
            let mut guard = CACHE.lock().unwrap();
            let cache = guard.get_or_insert_with(HashMap::new);
            cache.insert(path.to_path_buf(), entry.checksum.clone());
            return Ok((entry.checksum.clone(), None));
        }
    }

    // mtime changed or no cache entry — compute checksum
    let checksum = file_checksum(path)?;
    let new_entry = MtimeEntry {
        mtime_secs,
        mtime_nanos,
        checksum: checksum.clone(),
    };

    Ok((checksum, Some((path_str, new_entry))))
}

/// Flush a batch of dirty mtime entries in a single write transaction.
fn flush_mtime_entries(dirty: Vec<(String, MtimeEntry)>) -> Result<()> {
    if dirty.is_empty() {
        return Ok(());
    }
    let db_guard = get_mtime_db()?;
    let db = db_guard.as_ref().context("Mtime database not available")?;
    let write_txn = db.begin_write().context("Failed to begin write transaction for mtime cache")?;
    {
        let mut table = write_txn.open_table(MTIME_TABLE)
            .context("Failed to open mtime cache table")?;
        for (path_str, entry) in &dirty {
            let value = serde_json::to_vec(entry)
                .context("Failed to serialize mtime entry")?;
            table.insert(path_str.as_str(), value.as_slice())
                .context("Failed to insert mtime entry")?;
        }
    }
    ctx!(write_txn.commit(), "Failed to commit mtime cache entries")?;
    Ok(())
}

/// Hash a list of individual checksums into a single combined SHA-256 checksum.
fn hash_checksums(checksums: &[String]) -> String {
    let combined = checksums.join(":");
    bytes_checksum(combined.as_bytes())
}

/// Get the combined input checksum for a list of input files, using mtime
/// pre-check to avoid re-reading unchanged files across builds.
pub(crate) fn combined_input_checksum(inputs: &[PathBuf]) -> Result<String> {
    let mtime_enabled = MTIME_ENABLED.load(std::sync::atomic::Ordering::Relaxed);

    let mut checksums = Vec::with_capacity(inputs.len());
    let mut dirty_entries = Vec::new();

    for input in inputs {
        if input.exists() {
            if mtime_enabled {
                let (checksum, dirty) = fast_checksum(input)?;
                checksums.push(checksum);
                if let Some(entry) = dirty {
                    dirty_entries.push(entry);
                }
            } else {
                checksums.push(file_checksum(input)?);
            }
        } else {
            checksums.push(format!("MISSING:{}", input.display()));
        }
    }

    if mtime_enabled {
        flush_mtime_entries(dirty_entries)?;
    }

    Ok(hash_checksums(&checksums))
}

/// Invalidate a cached checksum for a path (e.g., after writing a new output file).
#[allow(dead_code)]
pub(crate) fn invalidate(path: &Path) {
    if let Ok(mut guard) = CACHE.lock() {
        if let Some(cache) = guard.as_mut() {
            cache.remove(path);
        }
    }
}

/// Clear the entire in-memory checksum cache.
#[allow(dead_code)]
pub(crate) fn clear_cache() {
    if let Ok(mut guard) = CACHE.lock() {
        *guard = None;
    }
}

/// Calculate SHA-256 checksum of a byte slice. Not cached.
pub(crate) fn bytes_checksum(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}
