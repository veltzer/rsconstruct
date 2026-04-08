use anyhow::{Context, Result};
use sha2::{Sha256, Digest};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Base directory for webcache files.
const CACHE_DIR: &str = ".rsconstruct/webcache";

/// An entry in the webcache.
pub struct CacheEntry {
    pub url_hash: String,
    pub size: u64,
    pub modified: Option<SystemTime>,
}

/// Return the cache file path for a given URL, using a 2-char prefix directory (like git objects).
fn cache_path(url: &str) -> (String, PathBuf) {
    let hash = hex::encode(Sha256::digest(url.as_bytes()));
    let prefix = &hash[..2];
    let rest = &hash[2..];
    let path = Path::new(CACHE_DIR).join(prefix).join(rest);
    (hash, path)
}

/// Fetch URL content, returning cached content if available.
/// On first fetch, the response is stored on disk under `.rsconstruct/webcache/`.
pub fn fetch(url: &str) -> Result<String> {
    let (_hash, path) = cache_path(url);

    if path.exists() {
        return fs::read_to_string(&path)
            .with_context(|| format!("Failed to read cached file {}", path.display()));
    }

    let body = ureq::get(url)
        .call()
        .with_context(|| format!("Failed to fetch {url}"))?
        .body_mut()
        .read_to_string()
        .with_context(|| format!("Failed to read response body from {url}"))?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create cache directory {}", parent.display()))?;
    }
    fs::write(&path, &body)
        .with_context(|| format!("Failed to write cache file {}", path.display()))?;

    Ok(body)
}

/// Delete all webcache files. Returns the number of files removed.
pub fn clear() -> Result<usize> {
    let cache_dir = Path::new(CACHE_DIR);
    if !cache_dir.exists() {
        return Ok(0);
    }
    let mut count = 0;
    for prefix_entry in fs::read_dir(cache_dir)? {
        let prefix_entry = prefix_entry?;
        let prefix_path = prefix_entry.path();
        if prefix_path.is_dir() {
            for file_entry in fs::read_dir(&prefix_path)? {
                let file_entry = file_entry?;
                if file_entry.path().is_file() {
                    fs::remove_file(file_entry.path())?;
                    count += 1;
                }
            }
            // Remove the now-empty prefix directory
            let _ = fs::remove_dir(&prefix_path);
        }
    }
    Ok(count)
}

/// List all cache entries with hash, size, and modified time.
pub fn list() -> Result<Vec<CacheEntry>> {
    let cache_dir = Path::new(CACHE_DIR);
    if !cache_dir.exists() {
        return Ok(Vec::new());
    }
    let mut entries = Vec::new();
    for prefix_entry in fs::read_dir(cache_dir)? {
        let prefix_entry = prefix_entry?;
        let prefix_path = prefix_entry.path();
        if !prefix_path.is_dir() {
            continue;
        }
        let prefix_name = prefix_entry.file_name();
        let prefix_str = prefix_name.to_string_lossy();
        for file_entry in fs::read_dir(&prefix_path)? {
            let file_entry = file_entry?;
            if !file_entry.path().is_file() {
                continue;
            }
            let file_name = file_entry.file_name();
            let rest = file_name.to_string_lossy();
            let url_hash = format!("{prefix_str}{rest}");
            let metadata = file_entry.metadata()?;
            entries.push(CacheEntry {
                url_hash,
                size: metadata.len(),
                modified: metadata.modified().ok(),
            });
        }
    }
    Ok(entries)
}

/// Return (total_bytes, entry_count) for the webcache.
pub fn stats() -> Result<(u64, usize)> {
    let entries = list()?;
    let total_bytes: u64 = entries.iter().map(|e| e.size).sum();
    let count = entries.len();
    Ok((total_bytes, count))
}
