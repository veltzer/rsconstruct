use anyhow::{Result, Context};
use std::fs;
use std::path::PathBuf;
use crate::checksum::ChecksumCache;
use crate::template::TemplateProcessor;

const CACHE_FILE: &str = ".rsb_cache.json";

pub struct Builder {
    project_root: PathBuf,
    checksum_cache: ChecksumCache,
    cache_file_path: PathBuf,
}

impl Builder {
    pub fn new() -> Result<Self> {
        let project_root = std::env::current_dir()?;
        let cache_file_path = project_root.join(CACHE_FILE);

        let checksum_cache = ChecksumCache::load_from_file(&cache_file_path)
            .unwrap_or_else(|_| ChecksumCache::new());

        Ok(Self {
            project_root,
            checksum_cache,
            cache_file_path,
        })
    }

    /// Execute an incremental build
    pub fn build(&mut self, force: bool) -> Result<()> {
        println!("Starting {} build...", if force { "forced" } else { "incremental" });

        // Process templates
        let templates_dir = self.project_root.join("templates");
        let output_dir = self.project_root.clone();

        if templates_dir.exists() {
            let mut processor = TemplateProcessor::new(templates_dir.clone(), output_dir)?;
            processor.process_all(&mut self.checksum_cache, force)?;
        } else {
            println!("No templates directory found, skipping template processing");
        }

        // Save checksum cache
        self.save_cache()?;

        println!("Build completed successfully!");
        Ok(())
    }

    /// Clean all build artifacts
    pub fn clean(&mut self) -> Result<()> {
        println!("Cleaning build artifacts...");

        // Clear checksum cache
        self.checksum_cache.clear();

        // Remove cache file
        if self.cache_file_path.exists() {
            fs::remove_file(&self.cache_file_path)
                .context("Failed to remove cache file")?;
            println!("Removed cache file: {}", self.cache_file_path.display());
        }

        // Clean generated files from templates
        let templates_dir = self.project_root.join("templates");
        if templates_dir.exists() {
            for entry in fs::read_dir(&templates_dir)? {
                let entry = entry?;
                let path = entry.path();

                if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("tera") {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        let output_file = self.project_root.join(stem);
                        if output_file.exists() && output_file.is_file() {
                            fs::remove_file(&output_file)?;
                            println!("Removed generated file: {}", output_file.display());
                        }
                    }
                }
            }
        }

        println!("Clean completed!");
        Ok(())
    }

    fn save_cache(&self) -> Result<()> {
        self.checksum_cache.save_to_file(&self.cache_file_path)?;
        Ok(())
    }
}