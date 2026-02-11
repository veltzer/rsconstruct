use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

/// Calculate SHA-256 checksum of a file's contents.
pub(crate) fn file_checksum(path: &Path) -> Result<String> {
    let contents = fs::read(path)
        .with_context(|| format!("Failed to read file for checksum: {}", path.display()))?;
    Ok(hex::encode(Sha256::digest(&contents)))
}

/// Calculate SHA-256 checksum of a byte slice.
pub(crate) fn bytes_checksum(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}
