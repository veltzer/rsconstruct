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

    pub fn update_checksum(&mut self, file_path: &Path) -> Result<()> {
        let checksum = Self::calculate_checksum(file_path)?;
        self.checksums.insert(file_path.to_path_buf(), checksum);
        Ok(())
    }

    pub fn clear(&mut self) {
        self.checksums.clear();
    }
}