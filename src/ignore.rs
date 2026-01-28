use anyhow::{Context, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};
use std::fs;
use std::path::{Path, PathBuf};

const IGNORE_FILE: &str = ".rsbignore";

pub struct IgnoreRules {
    globset: Option<GlobSet>,
    project_root: PathBuf,
}

impl IgnoreRules {
    /// Load ignore rules from `.rsbignore` in the project root.
    /// Returns Ok with empty rules if the file doesn't exist.
    pub fn load(project_root: &Path) -> Result<Self> {
        let ignore_path = project_root.join(IGNORE_FILE);
        if !ignore_path.exists() {
            return Ok(Self::empty(project_root));
        }

        let content = fs::read_to_string(&ignore_path)
            .with_context(|| format!("Failed to read {}", ignore_path.display()))?;

        let mut builder = GlobSetBuilder::new();
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let glob = Glob::new(trimmed)
                .with_context(|| format!("Invalid glob pattern in {}: {}", IGNORE_FILE, trimmed))?;
            builder.add(glob);
        }

        let globset = builder
            .build()
            .context("Failed to build ignore glob set")?;

        Ok(Self {
            globset: Some(globset),
            project_root: project_root.to_path_buf(),
        })
    }

    /// Create empty rules (no patterns match).
    pub fn empty(project_root: &Path) -> Self {
        Self {
            globset: None,
            project_root: project_root.to_path_buf(),
        }
    }

    /// Check whether a path should be ignored.
    /// The path is tested relative to the project root.
    pub fn is_ignored(&self, path: &Path) -> bool {
        let globset = match &self.globset {
            Some(gs) => gs,
            None => return false,
        };

        let relative = path
            .strip_prefix(&self.project_root)
            .unwrap_or(path);

        globset.is_match(relative)
    }
}
