use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use walkdir::WalkDir;

use crate::config::{CcConfig, CpplintConfig};
use crate::graph::{BuildGraph, Product};
use crate::ignore::IgnoreRules;
use super::ProductDiscovery;

const CPPLINT_STUB_DIR: &str = "out/cpplint";

pub struct Cpplinter {
    project_root: PathBuf,
    cpplint_config: CpplintConfig,
    cc_config: CcConfig,
    stub_dir: PathBuf,
    ignore_rules: Arc<IgnoreRules>,
}

impl Cpplinter {
    pub fn new(project_root: PathBuf, cpplint_config: CpplintConfig, cc_config: CcConfig, ignore_rules: Arc<IgnoreRules>) -> Self {
        let stub_dir = project_root.join(CPPLINT_STUB_DIR);
        Self {
            project_root,
            cpplint_config,
            cc_config,
            stub_dir,
            ignore_rules,
        }
    }

    /// Check if C/C++ linting should be enabled
    fn should_lint(&self) -> bool {
        let source_dir = self.project_root.join(&self.cc_config.source_dir);
        source_dir.exists()
    }

    /// Find all C/C++ source files that should be checked
    fn find_source_files(&self) -> Vec<PathBuf> {
        let source_dir = self.project_root.join(&self.cc_config.source_dir);
        if !source_dir.exists() {
            return Vec::new();
        }

        let mut files: Vec<PathBuf> = WalkDir::new(&source_dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| {
                let path = e.path();

                // Skip common non-source directories
                let path_str = path.to_string_lossy();
                if path_str.contains("/.git/")
                    || path_str.contains("/out/")
                    || path_str.contains("/build/")
                    || path_str.contains("/dist/")
                {
                    return false;
                }

                matches!(
                    path.extension().and_then(|s| s.to_str()),
                    Some("c") | Some("cc")
                )
            })
            .map(|e| e.path().to_path_buf())
            .filter(|p| !self.ignore_rules.is_ignored(p))
            .collect();
        files.sort();
        files
    }

    /// Get stub path for a C/C++ source file
    fn get_stub_path(&self, source_file: &Path) -> PathBuf {
        let relative_path = source_file
            .strip_prefix(&self.project_root)
            .unwrap_or(source_file);
        let stub_name = format!(
            "{}.cpplint",
            relative_path.display().to_string().replace(['/', '\\'], "_")
        );
        self.stub_dir.join(stub_name)
    }

    /// Run checker on a single file and create stub
    fn check_file(&self, source_file: &Path, stub_path: &Path) -> Result<()> {
        let mut cmd = Command::new(&self.cpplint_config.checker);

        cmd.arg("--error-exitcode=1");
        // Enable useful checks but exclude 'information' severity which produces
        // non-actionable noise (e.g. normalCheckLevelMaxBranches)
        cmd.arg("--enable=warning,style,performance,portability");
        cmd.arg("--suppress=missingIncludeSystem");

        // Add any configured arguments
        for arg in &self.cpplint_config.args {
            cmd.arg(arg);
        }

        cmd.arg(source_file);
        cmd.current_dir(&self.project_root);

        let output = cmd
            .output()
            .context(format!("Failed to run checker: {}", self.cpplint_config.checker))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            return Err(anyhow::anyhow!(
                "C/C++ checking failed:\n{}{}",
                stdout,
                stderr
            ));
        }

        // Create stub file on success
        if let Some(parent) = stub_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(stub_path, "checked").context("Failed to create cpplint stub file")?;

        Ok(())
    }
}

impl ProductDiscovery for Cpplinter {
    fn discover(&self, graph: &mut BuildGraph) -> Result<()> {
        if !self.should_lint() {
            return Ok(());
        }

        let source_files = self.find_source_files();

        for source_file in source_files {
            let stub_path = self.get_stub_path(&source_file);
            graph.add_product(
                vec![source_file],
                vec![stub_path],
                "cpplint",
            );
        }

        Ok(())
    }

    fn execute(&self, product: &Product) -> Result<()> {
        if product.inputs.len() != 1 || product.outputs.len() != 1 {
            anyhow::bail!("Cpplint product must have exactly one input and one output");
        }

        // Ensure stub directory exists
        if !self.stub_dir.exists() {
            fs::create_dir_all(&self.stub_dir)
                .context("Failed to create cpplint stub directory")?;
        }

        self.check_file(&product.inputs[0], &product.outputs[0])
    }

    fn clean(&self, product: &Product) -> Result<()> {
        for output in &product.outputs {
            if output.exists() {
                fs::remove_file(output)?;
                println!("Removed cpplint stub: {}", output.display());
            }
        }
        Ok(())
    }
}
