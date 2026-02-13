use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::config::ScanConfig;

#[derive(Debug)]
pub struct FileIndex {
    files: Vec<PathBuf>,
}

#[cfg(test)]
impl FileIndex {
    /// Create a FileIndex from an explicit list of paths (for testing).
    fn from_paths(mut files: Vec<PathBuf>) -> Self {
        files.sort();
        Self { files }
    }
}

impl FileIndex {
    /// Build a file index by walking the current directory once.
    /// Uses `ignore::WalkBuilder` which natively handles `.gitignore` and
    /// `.rsbignore` (via `add_custom_ignore_filename`).
    /// All paths are stored relative to project root (cwd).
    pub fn build() -> Result<Self> {
        let walker = ignore::WalkBuilder::new(".")
            .add_custom_ignore_filename(".rsbignore")
            .hidden(false) // don't skip hidden files by default (let .gitignore handle it)
            .build();

        let mut files: Vec<PathBuf> = Vec::new();
        for entry in walker {
            let entry = entry.context("Failed to read directory entry during file indexing")?;
            if entry.file_type().is_some_and(|ft| ft.is_file()) {
                let path = entry.into_path();
                // Store relative paths (strip "./" prefix)
                let relative = path.strip_prefix(".")
                    .unwrap_or(&path)
                    .to_path_buf();
                files.push(relative);
            }
        }
        files.sort();

        Ok(Self { files })
    }

    /// Query the index for files matching the given criteria.
    /// All paths in the index are relative to project root.
    ///
    /// - `root`: only include files under this directory (relative path, e.g., "src" or "")
    /// - `extensions`: file extensions to match (e.g., `[".py", ".pyi"]`)
    /// - `exclude_dirs`: directory path segments to skip (e.g., `["/.git/", "/out/"]`)
    /// - `exclude_files`: file names to skip (e.g., `["setup.py"]`)
    /// - `exclude_paths`: paths relative to project root to skip (e.g., `["Makefile"]`)
    pub fn query(
        &self,
        root: &Path,
        extensions: &[&str],
        exclude_dirs: &[&str],
        exclude_files: &[&str],
        exclude_paths: &[&str],
    ) -> Vec<PathBuf> {
        self.files
            .iter()
            .filter(|path| {
                // Must be under root (root is relative, e.g., "src" or "")
                // Empty root or "." means match all
                let root_str = root.to_string_lossy();
                if !root_str.is_empty() && root_str != "."
                    && !path.starts_with(root) {
                        return false;
                    }

                // Check exclude dirs
                if !exclude_dirs.is_empty() {
                    let path_str = path.to_string_lossy();
                    if exclude_dirs.iter().any(|dir| path_str.contains(dir)) {
                        return false;
                    }
                }

                // Check extension match
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if !extensions.iter().any(|ext| name.ends_with(ext)) {
                    return false;
                }

                // Check exclude files
                if !exclude_files.is_empty() && exclude_files.contains(&name) {
                    return false;
                }

                // Check exclude paths (paths are already relative)
                if !exclude_paths.is_empty() {
                    let path_str = path.to_string_lossy();
                    if exclude_paths.iter().any(|p| *p == path_str) {
                        return false;
                    }
                }

                true
            })
            .cloned()
            .collect()
    }

    /// Convenience wrapper using `ScanConfig` fields.
    /// Returns relative paths.
    ///
    /// - `scan`: processor scan configuration
    /// - `recursive`: if false, only include files at depth 1 from the scan root
    pub fn scan(
        &self,
        scan: &ScanConfig,
        recursive: bool,
    ) -> Vec<PathBuf> {
        let dir = scan.scan_dir();
        let root = PathBuf::from(dir);
        let ext_refs: Vec<&str> = scan.extensions().iter().map(|s| s.as_str()).collect();
        let exclude_dir_refs: Vec<&str> = scan.exclude_dirs().iter().map(|s| s.as_str()).collect();
        let exclude_file_refs: Vec<&str> = scan.exclude_files().iter().map(|s| s.as_str()).collect();
        let exclude_path_refs: Vec<&str> = scan.exclude_paths().iter().map(|s| s.as_str()).collect();
        let mut results = self.query(&root, &ext_refs, &exclude_dir_refs, &exclude_file_refs, &exclude_path_refs);

        if !recursive {
            // Filter to depth 1 from scan root: keep only files whose path has
            // exactly one more component than the root.
            // e.g. root="src" keeps "src/main.c" but not "src/sub/foo.c"
            //      root=""    keeps "README.md"   but not "sub/foo.c"
            let root_depth = root.components().count();
            results.retain(|path| {
                path.components().count() == root_depth + 1
            });
        }

        results
    }

    /// Check if the index contains any file with the given extension.
    /// Extension should include the dot, e.g., ".py", ".c".
    pub fn has_extension(&self, ext: &str) -> bool {
        self.files.iter().any(|path| {
            path.file_name()
                .and_then(|n| n.to_str())
                .map(|name| name.ends_with(ext))
                .unwrap_or(false)
        })
    }

    /// Check if a specific path exists in the index.
    /// Uses binary search since the file list is sorted.
    pub fn contains(&self, path: &Path) -> bool {
        self.files.binary_search_by(|p| p.as_path().cmp(path)).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_index() -> FileIndex {
        FileIndex::from_paths(vec![
            "src/main.c".into(),
            "src/lib.c".into(),
            "src/util/helper.c".into(),
            "tests/test_main.py".into(),
            "tests/test_lib.py".into(),
            "README.md".into(),
            "Makefile".into(),
            "out/build/app.o".into(),
        ])
    }

    #[test]
    fn contains_finds_existing_path() {
        let idx = sample_index();
        assert!(idx.contains(Path::new("src/main.c")));
        assert!(idx.contains(Path::new("README.md")));
    }

    #[test]
    fn contains_rejects_missing_path() {
        let idx = sample_index();
        assert!(!idx.contains(Path::new("nonexistent.c")));
        assert!(!idx.contains(Path::new("src/missing.c")));
    }

    #[test]
    fn has_extension_finds_existing() {
        let idx = sample_index();
        assert!(idx.has_extension(".c"));
        assert!(idx.has_extension(".py"));
        assert!(idx.has_extension(".md"));
    }

    #[test]
    fn has_extension_rejects_missing() {
        let idx = sample_index();
        assert!(!idx.has_extension(".rs"));
        assert!(!idx.has_extension(".java"));
    }

    #[test]
    fn query_filters_by_extension() {
        let idx = sample_index();
        let results = idx.query(Path::new(""), &[".c"], &[], &[], &[]);
        assert_eq!(results.len(), 3); // main.c, lib.c, helper.c
        assert!(results.iter().all(|p| p.to_string_lossy().ends_with(".c")));
    }

    #[test]
    fn query_filters_by_root() {
        let idx = sample_index();
        let results = idx.query(Path::new("tests"), &[".py"], &[], &[], &[]);
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|p| p.starts_with("tests")));
    }

    #[test]
    fn query_excludes_dirs() {
        let idx = sample_index();
        let results = idx.query(Path::new(""), &[".c", ".o"], &["/util/"], &[], &[]);
        assert!(!results.iter().any(|p| p.to_string_lossy().contains("/util/")));
    }

    #[test]
    fn query_excludes_files() {
        let idx = sample_index();
        let results = idx.query(Path::new(""), &[".c"], &[], &["lib.c"], &[]);
        assert!(!results.iter().any(|p| p.file_name().unwrap() == "lib.c"));
        assert!(results.iter().any(|p| p.file_name().unwrap() == "main.c"));
    }

    #[test]
    fn query_excludes_paths() {
        let idx = sample_index();
        let results = idx.query(Path::new(""), &[".c"], &[], &[], &["src/main.c"]);
        assert!(!results.contains(&PathBuf::from("src/main.c")));
        assert!(results.contains(&PathBuf::from("src/lib.c")));
    }

    #[test]
    fn query_empty_root_matches_all() {
        let idx = sample_index();
        let results = idx.query(Path::new(""), &[".md"], &[], &[], &[]);
        assert_eq!(results, vec![PathBuf::from("README.md")]);
    }
}

