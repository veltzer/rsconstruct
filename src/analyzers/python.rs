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

use crate::config::PythonAnalyzerConfig;
use crate::errors;
use crate::file_index::FileIndex;
use crate::graph::Product;

use super::{DepAnalyzer, ScanResult};

/// One import statement: `import module` (no names) or
/// `from module import names…`.
struct Import {
    module: String,
    /// The names after `from module import`; each may be a submodule.
    names: Vec<String>,
}

/// The first word of each comma-separated item (`a as b` → `a`), skipping
/// `*` and blanks.
fn import_items(list: &str) -> impl Iterator<Item = String> + '_ {
    list.split(',')
        .filter_map(|item| item.split_whitespace().next())
        .filter(|name| *name != "*" && *name != "\\")
        .map(str::to_string)
}

/// Parse the `import` / `from X import …` statements of Python source,
/// including parenthesized multi-line name lists. Comments are skipped.
fn parse_imports(content: &str) -> Vec<Import> {
    static FROM_RE: OnceLock<Regex> = OnceLock::new();
    static IMPORT_RE: OnceLock<Regex> = OnceLock::new();
    let from_re = FROM_RE.get_or_init(|| {
        Regex::new(r"^\s*from\s+(\S+)\s+import\s+(.*)$").expect(errors::INVALID_REGEX)
    });
    let import_re =
        IMPORT_RE.get_or_init(|| Regex::new(r"^\s*import\s+(.*)$").expect(errors::INVALID_REGEX));

    let mut imports: Vec<Import> = Vec::new();
    // Inside `from X import (` … `)`: the names continue on later lines.
    let mut in_parens = false;
    for line in content.lines() {
        let code = line.split('#').next().unwrap_or("");
        if in_parens {
            let (names, closed) = code
                .split_once(')')
                .map_or((code, false), |(n, _)| (n, true));
            if let Some(last) = imports.last_mut() {
                last.names.extend(import_items(names));
            }
            in_parens = !closed;
        } else if let Some(caps) = from_re.captures(code) {
            let rest = caps[2].trim();
            let names = match rest.strip_prefix('(') {
                Some(inner) => {
                    if let Some((names, _)) = inner.split_once(')') {
                        names
                    } else {
                        in_parens = true;
                        inner
                    }
                }
                None => rest,
            };
            imports.push(Import {
                module: caps[1].to_string(),
                names: import_items(names).collect(),
            });
        } else if let Some(caps) = import_re.captures(code) {
            imports.extend(import_items(&caps[1]).map(|module| Import {
                module,
                names: Vec::new(),
            }));
        }
    }
    imports
}

/// Scan a Python source file for `import` / `from X import ...` statements and
/// return the module names referenced. Comments are skipped. The caller
/// decides how to classify each name (local, stdlib, third-party).
pub fn scan_python_imports(source: &Path) -> Result<Vec<String>> {
    let content = crate::errors::ctx(
        fs::read_to_string(source),
        &format!("Failed to read Python source: {}", source.display()),
    )?;
    Ok(parse_imports(&content)
        .into_iter()
        .map(|import| import.module)
        .collect())
}

/// Python dependency analyzer that scans source files for import statements.
pub struct PythonDepAnalyzer {
    config: PythonAnalyzerConfig,
}

impl PythonDepAnalyzer {
    pub const fn new(config: PythonAnalyzerConfig) -> Self {
        Self { config }
    }

    /// Scan `source` for transitive imports: every local Python file it
    /// imports, the files those import, and so on (excluding the source
    /// itself), plus every path probed and found missing along the way.
    /// Running `a.py` runs the modules it imports, so a change two imports
    /// away changes what `a.py` does. Stdlib and third-party modules are
    /// filtered out by `resolve_module`. A module another product has yet
    /// to generate is a dependency but cannot be read yet; the source is
    /// analyzed again once it is written (`Analysis::reanalyze`).
    fn scan_imports(&self, source: &Path, file_index: &FileIndex) -> Result<ScanResult> {
        let mut imports = Vec::new();
        let mut absent = Vec::new();
        let mut seen: HashSet<PathBuf> = HashSet::from([source.to_path_buf()]);
        let mut queue: Vec<PathBuf> = vec![source.to_path_buf()];
        while let Some(file) = queue.pop() {
            let content = errors::ctx(
                fs::read_to_string(&file),
                &format!("Failed to read Python source: {}", file.display()),
            )?;
            for import in parse_imports(&content) {
                // `from pkg import name` imports the submodule `pkg.name`
                // when there is one; a name that is not a module resolves
                // nowhere and adds only its probes.
                let submodules = import
                    .names
                    .iter()
                    .map(|n| format!("{}.{n}", import.module));
                for module in std::iter::once(import.module.clone()).chain(submodules) {
                    // Each module resolves relative to the file that imports it.
                    let (found, probed) = self.resolve_module(&file, &module, file_index);
                    absent.extend(probed);
                    for path in found {
                        if seen.insert(path.clone()) {
                            imports.push(path.clone());
                            if path.is_file() {
                                queue.push(path);
                            }
                        }
                    }
                }
            }
        }
        absent.sort();
        absent.dedup();
        Ok(ScanResult::deps(imports, absent))
    }

    /// Resolve a module name to the local files importing it runs — the
    /// module itself and the `__init__.py` of each package above it — along
    /// with the paths probed and found missing. Nothing is found for
    /// stdlib/external modules, whose candidates are all absent, so a local
    /// module of that name appearing later is noticed.
    ///
    /// Roots are tried in order: the importing file's directory, the project
    /// root, then `search_paths`. The first root holding `module.py` or
    /// `module/__init__.py` wins.
    fn resolve_module(
        &self,
        importer: &Path,
        module: &str,
        file_index: &FileIndex,
    ) -> (Vec<PathBuf>, Vec<PathBuf>) {
        let parts: Vec<&str> = module.split('.').collect();
        if parts.iter().any(|p| p.is_empty()) {
            return (Vec::new(), Vec::new());
        }
        let module_path = parts.join("/");
        // In the file index, or on disk (cwd is project root).
        let exists = |p: &Path| file_index.contains(p) || p.is_file();

        let roots = std::iter::once(crate::processor::parent_dir(importer).to_path_buf())
            .chain(std::iter::once(PathBuf::new()))
            .chain(self.config.search_paths.iter().map(PathBuf::from));
        let mut probed = Vec::new();
        for root in roots {
            for candidate in [
                root.join(format!("{module_path}.py")),
                root.join(&module_path).join("__init__.py"),
            ] {
                if !exists(&candidate) {
                    probed.push(candidate);
                    continue;
                }
                // Importing `a.b.c` first runs `a/__init__.py` and
                // `a/b/__init__.py`.
                let mut found = vec![candidate];
                for depth in 1..parts.len() {
                    let init = root.join(parts[..depth].join("/")).join("__init__.py");
                    if exists(&init) {
                        found.push(init);
                    } else {
                        probed.push(init);
                    }
                }
                return (found, probed);
            }
        }
        (Vec::new(), probed)
    }
}

impl DepAnalyzer for PythonDepAnalyzer {
    fn description(&self) -> &'static str {
        "Scan Python source files for import dependencies"
    }

    fn enabled(&self) -> bool {
        self.config.enabled
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        // Check if there are any Python files
        file_index.has_extension(".py")
    }

    fn match_product(&self, p: &Product) -> Option<PathBuf> {
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
    }

    fn scan(
        &self,
        _ctx: &crate::build_context::BuildContext,
        source: &Path,
        file_index: &FileIndex,
    ) -> Result<ScanResult> {
        self.scan_imports(source, file_index)
    }

    fn fingerprint_parts(&self, _ctx: &crate::build_context::BuildContext) -> Result<Vec<String>> {
        super::config_fingerprint(&self.config)
    }
}

inventory::submit! {
    crate::registries::AnalyzerPlugin {
        name: "python",
        description: "Scan Python files for local import dependencies",
        is_native: true,
        create: |toml_value, _| {
            let cfg: PythonAnalyzerConfig = toml::from_str(&toml::to_string(toml_value)?)?;
            Ok(Box::new(PythonDepAnalyzer::new(cfg)))
        },
        defconfig_toml: || {
            toml::to_string_pretty(&PythonAnalyzerConfig::default()).ok()
        },
        known_fields: crate::registries::typed_known_fields::<PythonAnalyzerConfig>,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(src: &str) -> Vec<(String, Vec<String>)> {
        parse_imports(src)
            .into_iter()
            .map(|i| (i.module, i.names))
            .collect()
    }

    fn owned(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn parse_imports_statement_forms() {
        let src = "\
import os, sys  # trailing comment
import a.b as c
# import commented
    import indented
from pkg.sub import x, y as z
from pkg import *
from multi import (
    one,
    two,  # comment
)
from inline import (p, q)
";
        assert_eq!(
            parsed(src),
            vec![
                ("os".to_string(), vec![]),
                ("sys".to_string(), vec![]),
                ("a.b".to_string(), vec![]),
                ("indented".to_string(), vec![]),
                ("pkg.sub".to_string(), owned(&["x", "y"])),
                ("pkg".to_string(), vec![]),
                ("multi".to_string(), owned(&["one", "two"])),
                ("inline".to_string(), owned(&["p", "q"])),
            ]
        );
    }
}
