use anyhow::Result;
use sha2::{Sha256, Digest};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct ChecksumCache {
    checksums: HashMap<PathBuf, String>,
}

impl ChecksumCache {
    pub fn new() -> Self {
        Self {
            checksums: HashMap::new(),
        }
    }

    pub fn load_from_file(path: &Path) -> Result<Self> {
        if path.exists() {
            let content = fs::read_to_string(path)?;
            Ok(serde_json::from_str(&content)?)
        } else {
            Ok(Self::new())
        }
    }

    pub fn save_to_file(&self, path: &Path) -> Result<()> {
        let content = serde_json::to_string_pretty(&self)?;
        fs::write(path, content)?;
        Ok(())
    }

    pub fn calculate_checksum(file_path: &Path) -> Result<String> {
        let contents = fs::read(file_path)?;
        let mut hasher = Sha256::new();
        hasher.update(contents);
        let result = hasher.finalize();
        Ok(hex::encode(result))
    }

    pub fn has_changed(&mut self, file_path: &Path) -> Result<bool> {
        let current_checksum = Self::calculate_checksum(file_path)?;

        if let Some(stored_checksum) = self.checksums.get(file_path) {
            if *stored_checksum == current_checksum {
                return Ok(false);
            }
        }

        self.checksums.insert(file_path.to_path_buf(), current_checksum);
        Ok(true)
    }


    pub fn clear(&mut self) {
        self.checksums.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_checksum_calculation() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");

        fs::write(&file_path, "Hello, World!").unwrap();

        let checksum1 = ChecksumCache::calculate_checksum(&file_path).unwrap();
        let checksum2 = ChecksumCache::calculate_checksum(&file_path).unwrap();

        // Same content should produce same checksum
        assert_eq!(checksum1, checksum2);

        // Checksum should be a valid hex string
        assert_eq!(checksum1.len(), 64); // SHA256 produces 64 hex characters
    }

    #[test]
    fn test_change_detection() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");

        fs::write(&file_path, "Initial content").unwrap();

        let mut cache = ChecksumCache::new();

        // First check should report as changed
        assert!(cache.has_changed(&file_path).unwrap());

        // Second check without file modification should report as unchanged
        assert!(!cache.has_changed(&file_path).unwrap());

        // Modify the file
        fs::write(&file_path, "Modified content").unwrap();

        // Should now report as changed
        assert!(cache.has_changed(&file_path).unwrap());
    }

    #[test]
    fn test_cache_persistence() {
        let temp_dir = TempDir::new().unwrap();
        let cache_path = temp_dir.path().join("cache.json");
        let file_path = temp_dir.path().join("test.txt");

        fs::write(&file_path, "Test content").unwrap();

        // Create cache and track a file
        let mut cache1 = ChecksumCache::new();
        cache1.has_changed(&file_path).unwrap();
        cache1.save_to_file(&cache_path).unwrap();

        // Load cache from file
        let mut cache2 = ChecksumCache::load_from_file(&cache_path).unwrap();

        // Should recognize the file as unchanged
        assert!(!cache2.has_changed(&file_path).unwrap());
    }

    #[test]
    fn test_clear() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");

        fs::write(&file_path, "Content").unwrap();

        let mut cache = ChecksumCache::new();
        cache.has_changed(&file_path).unwrap();

        // Cache should have the file
        assert!(!cache.checksums.is_empty());

        // Clear the cache
        cache.clear();

        // Cache should be empty
        assert!(cache.checksums.is_empty());

        // File should be detected as changed again
        assert!(cache.has_changed(&file_path).unwrap());
    }
}