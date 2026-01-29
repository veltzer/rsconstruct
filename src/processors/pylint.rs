use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use walkdir::WalkDir;

use crate::config::{PylintConfig, config_hash};
use crate::graph::{BuildGraph, Product};
use crate::ignore::IgnoreRules;
use super::ProductDiscovery;

const PYLINT_STUB_DIR: &str = "out/pylint";

pub struct Pylinter {
    project_root: PathBuf,
    pylint_config: PylintConfig,
    stub_dir: PathBuf,
    ignore_rules: Arc<IgnoreRules>,
}

impl Pylinter {
    pub fn new(project_root: PathBuf, pylint_config: PylintConfig, ignore_rules: Arc<IgnoreRules>) -> Self {
        let stub_dir = project_root.join(PYLINT_STUB_DIR);
        Self {
            project_root,
            pylint_config,
            stub_dir,
            ignore_rules,
        }
    }

    /// Check if linting should be enabled for this project
    fn should_lint(&self) -> bool {
        let pyproject_exists = self.project_root.join("pyproject.toml").exists();
        let tests_dir = self.project_root.join("tests");
        let tests_has_python = tests_dir.exists() && self.has_python_files(&tests_dir);

        pyproject_exists || tests_has_python
    }

    /// Find all Python files that should be linted
    fn find_python_files(&self) -> Vec<PathBuf> {
        if self.project_root.join("pyproject.toml").exists() {
            self.find_py_files_in_project()
        } else {
            let tests_dir = self.project_root.join("tests");
            if tests_dir.exists() {
                self.find_py_files_in_dir(&tests_dir)
            } else {
                Vec::new()
            }
        }
    }

    fn has_python_files(&self, dir: &Path) -> bool {
        WalkDir::new(dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .any(|e| e.path().extension().and_then(|s| s.to_str()) == Some("py"))
    }

    fn find_py_files_in_dir(&self, dir: &Path) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = WalkDir::new(dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("py"))
            .map(|e| e.path().to_path_buf())
            .filter(|p| !self.ignore_rules.is_ignored(p))
            .collect();
        files.sort();
        files
    }

    fn find_py_files_in_project(&self) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = WalkDir::new(&self.project_root)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| {
                let path = e.path();

                // Skip common non-source directories
                let path_str = path.to_string_lossy();
                if path_str.contains("/.venv/")
                    || path_str.contains("/__pycache__/")
                    || path_str.contains("/.git/")
                    || path_str.contains("/out/")
                    || path_str.contains("/node_modules/")
                    || path_str.contains("/.tox/")
                    || path_str.contains("/build/")
                    || path_str.contains("/dist/")
                    || path_str.contains("/.eggs/")
                {
                    return false;
                }

                path.extension().and_then(|s| s.to_str()) == Some("py")
            })
            .map(|e| e.path().to_path_buf())
            .filter(|p| !self.ignore_rules.is_ignored(p))
            .collect();
        files.sort();
        files
    }

    /// Get stub path for a Python file
    fn get_stub_path(&self, py_file: &Path) -> PathBuf {
        let relative_path = py_file
            .strip_prefix(&self.project_root)
            .unwrap_or(py_file);
        let stub_name = format!(
            "{}.pylint",
            relative_path.display().to_string().replace(['/', '\\'], "_")
        );
        self.stub_dir.join(stub_name)
    }

    /// Run linter on a single file and create stub
    fn lint_file(&self, py_file: &Path, stub_path: &Path) -> Result<()> {
        let mut cmd = Command::new(&self.pylint_config.linter);

        // Add check mode for ruff (don't auto-fix)
        if self.pylint_config.linter == "ruff" {
            cmd.arg("check");
        }

        // Add any configured arguments
        for arg in &self.pylint_config.args {
            cmd.arg(arg);
        }

        cmd.arg(py_file);
        cmd.current_dir(&self.project_root);

        let output = cmd
            .output()
            .context(format!("Failed to run linter: {}", self.pylint_config.linter))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            return Err(anyhow::anyhow!(
                "Python linting failed:\n{}{}",
                stdout,
                stderr
            ));
        }

        // Create stub file on success
        if let Some(parent) = stub_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(stub_path, "linted").context("Failed to create pylint stub file")?;

        Ok(())
    }
}

impl ProductDiscovery for Pylinter {
    fn discover(&self, graph: &mut BuildGraph) -> Result<()> {
        if !self.should_lint() {
            return Ok(());
        }

        let py_files = self.find_python_files();
        let config_hash = Some(config_hash(&self.pylint_config));

        for py_file in py_files {
            let stub_path = self.get_stub_path(&py_file);
            graph.add_product(
                vec![py_file],
                vec![stub_path],
                "pylint",
                config_hash.clone(),
            );
        }

        Ok(())
    }

    fn execute(&self, product: &Product) -> Result<()> {
        if product.inputs.len() != 1 || product.outputs.len() != 1 {
            anyhow::bail!("Pylint product must have exactly one input and one output");
        }

        // Ensure stub directory exists
        if !self.stub_dir.exists() {
            fs::create_dir_all(&self.stub_dir)
                .context("Failed to create pylint stub directory")?;
        }

        self.lint_file(&product.inputs[0], &product.outputs[0])
    }

    fn clean(&self, product: &Product) -> Result<()> {
        for output in &product.outputs {
            if output.exists() {
                fs::remove_file(output)?;
                println!("Removed pylint stub: {}", output.display());
            }
        }
        Ok(())
    }
}
