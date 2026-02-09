//! Python dependency analyzer for scanning import statements.
//!
//! Scans Python source files for import statements and adds dependencies
//! to products in the build graph.

use anyhow::Result;
use regex::Regex;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::deps_cache::DepsCache;
use crate::errors;
use crate::file_index::FileIndex;
use crate::graph::BuildGraph;

use super::DepAnalyzer;

/// Python dependency analyzer that scans source files for import statements.
pub struct PythonDepAnalyzer {
    project_root: PathBuf,
}

impl PythonDepAnalyzer {
    pub fn new(project_root: PathBuf) -> Self {
        Self { project_root }
    }

    /// Scan a Python file for import statements.
    /// Returns paths to local Python files that are imported.
    fn scan_imports(&self, source: &Path, file_index: &FileIndex) -> Result<Vec<PathBuf>> {
        let content = fs::read_to_string(source)?;
        let mut imports = Vec::new();
        let mut seen = HashSet::new();

        // Match: import foo, import foo.bar, from foo import bar, from foo.bar import baz
        // We capture the module path (before any 'import' keyword in 'from' statements)
        static IMPORT_RE: OnceLock<Regex> = OnceLock::new();
        let import_re = IMPORT_RE.get_or_init(|| Regex::new(r"^\s*(?:from\s+(\S+)\s+import|import\s+(\S+))").expect(errors::INVALID_REGEX));

        for line in content.lines() {
            // Skip comments
            let trimmed = line.trim();
            if trimmed.starts_with('#') {
                continue;
            }

            if let Some(caps) = import_re.captures(line) {
                // caps[1] is for "from X import" style
                // caps[2] is for "import X" style
                let module_path = caps.get(1).or_else(|| caps.get(2)).map(|m| m.as_str());

                if let Some(module) = module_path {
                    // Handle multiple imports: import foo, bar, baz
                    for part in module.split(',') {
                        let module_name = part.split_whitespace().next().unwrap_or("");
                        if module_name.is_empty() {
                            continue;
                        }

                        // Try to resolve the module to a local file
                        if let Some(path) = self.resolve_module(source, module_name, file_index)
                            && !seen.contains(&path) {
                                seen.insert(path.clone());
                                imports.push(path);
                            }
                    }
                }
            }
        }

        Ok(imports)
    }

    /// Try to resolve a Python module name to a local file path.
    /// Returns None for stdlib/external modules.
    fn resolve_module(&self, source: &Path, module: &str, file_index: &FileIndex) -> Option<PathBuf> {
        // Convert module.path to module/path
        let module_path = module.replace('.', "/");

        // Get the directory containing the source file
        let source_dir = source.parent().unwrap_or(Path::new("."));

        // Try various resolution strategies:
        // 1. Relative to source file: source_dir/module.py
        // 2. Relative to source file: source_dir/module/__init__.py
        // 3. From project root: module.py
        // 4. From project root: module/__init__.py

        let candidates = [
            // Relative paths
            source_dir.join(format!("{}.py", module_path)),
            source_dir.join(&module_path).join("__init__.py"),
            // Project root paths
            PathBuf::from(format!("{}.py", module_path)),
            PathBuf::from(&module_path).join("__init__.py"),
        ];

        for candidate in &candidates {
            // Check if this file exists in the file index
            if file_index.contains(candidate) {
                return Some(candidate.clone());
            }
            // Also check absolute path
            let abs_candidate = self.project_root.join(candidate);
            if abs_candidate.is_file() {
                return Some(candidate.clone());
            }
        }

        None
    }
}

impl DepAnalyzer for PythonDepAnalyzer {
    fn description(&self) -> &str {
        "Scan Python source files for import dependencies"
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        // Check if there are any Python files
        file_index.has_extension(".py")
    }

    fn analyze(&self, graph: &mut BuildGraph, deps_cache: &mut DepsCache, file_index: &FileIndex, verbose: bool) -> Result<()> {
        super::analyze_with_scanner(
            graph,
            deps_cache,
            "python",
            |p| {
                if p.inputs.is_empty() {
                    return None;
                }
                let source = &p.inputs[0];
                let ext = source.extension().and_then(|s| s.to_str()).unwrap_or("");
                if ext == "py" {
                    Some(source.clone())
                } else {
                    None
                }
            },
            |source| {
                // Python analyzer swallows scan errors (unwrap_or_default in original)
                Ok(self.scan_imports(source, file_index).unwrap_or_default())
            },
            verbose,
        )
    }
}
