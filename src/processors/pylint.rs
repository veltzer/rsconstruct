use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use crate::config::{PylintConfig, config_hash, resolve_extra_inputs};
use crate::graph::{BuildGraph, Product};
use crate::ignore::IgnoreRules;
use super::{ProductDiscovery, find_files, PYTHON_EXCLUDE_DIRS};

const PYLINT_STUB_DIR: &str = "out/pylint";

pub struct PylintProcessor {
    project_root: PathBuf,
    pylint_config: PylintConfig,
    stub_dir: PathBuf,
    ignore_rules: Arc<IgnoreRules>,
}

impl PylintProcessor {
    pub fn new(project_root: PathBuf, pylint_config: PylintConfig, ignore_rules: Arc<IgnoreRules>) -> Self {
        let stub_dir = project_root.join(PYLINT_STUB_DIR);
        Self {
            project_root,
            pylint_config,
            stub_dir,
            ignore_rules,
        }
    }

    /// Find all Python files that should be linted
    fn find_python_files(&self) -> Vec<PathBuf> {
        find_files(&self.project_root, &[".py"], PYTHON_EXCLUDE_DIRS, &self.ignore_rules, true)
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

    /// Run pylint on a single file and create stub
    fn lint_file(&self, py_file: &Path, stub_path: &Path) -> Result<()> {
        let mut cmd = Command::new("pylint");

        // Add any configured arguments
        for arg in &self.pylint_config.args {
            cmd.arg(arg);
        }

        cmd.arg(py_file);
        cmd.current_dir(&self.project_root);

        let output = cmd
            .output()
            .context("Failed to run pylint")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            return Err(anyhow::anyhow!(
                "Pylint failed:\n{}{}",
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

impl ProductDiscovery for PylintProcessor {
    fn auto_detect(&self) -> bool {
        !self.find_python_files().is_empty()
    }

    fn discover(&self, graph: &mut BuildGraph) -> Result<()> {
        let py_files = self.find_python_files();
        if py_files.is_empty() {
            return Ok(());
        }
        let config_hash = Some(config_hash(&self.pylint_config));
        let extra = resolve_extra_inputs(&self.project_root, &self.pylint_config.extra_inputs)?;

        for py_file in py_files {
            let stub_path = self.get_stub_path(&py_file);
            let mut inputs = vec![py_file];
            inputs.extend(extra.clone());
            graph.add_product(
                inputs,
                vec![stub_path],
                "pylint",
                config_hash.clone(),
            );
        }

        Ok(())
    }

    fn execute(&self, product: &Product) -> Result<()> {
        if product.inputs.is_empty() || product.outputs.len() != 1 {
            anyhow::bail!("Pylint product must have at least one input and exactly one output");
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
