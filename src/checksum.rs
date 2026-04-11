use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Global in-memory checksum cache. Avoids re-reading and re-hashing
/// the same file multiple times within a single build run.
static CACHE: Mutex<Option<HashMap<PathBuf, String>>> = Mutex::new(None);

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

/// Invalidate a cached checksum for a path (e.g., after writing a new output file).
#[allow(dead_code)]
pub(crate) fn invalidate(path: &Path) {
    if let Ok(mut guard) = CACHE.lock() {
        if let Some(cache) = guard.as_mut() {
            cache.remove(path);
        }
    }
}

/// Clear the entire checksum cache (e.g., between build runs).
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
