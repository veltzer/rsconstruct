use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::RuffConfig;
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use super::{ProductDiscovery, discover_stub_products, validate_stub_product, ensure_stub_dir, write_stub, clean_outputs, log_command, execute_lint_batch};

const RUFF_STUB_DIR: &str = "out/ruff";

pub struct RuffProcessor {
    project_root: PathBuf,
    ruff_config: RuffConfig,
    stub_dir: PathBuf,
}

impl RuffProcessor {
    pub fn new(project_root: PathBuf, ruff_config: RuffConfig) -> Self {
        let stub_dir = project_root.join(RUFF_STUB_DIR);
        Self {
            project_root,
            ruff_config,
            stub_dir,
        }
    }

    /// Run the configured linter on a single file and create stub
    fn lint_file(&self, py_file: &Path, stub_path: &Path) -> Result<()> {
        let linter = &self.ruff_config.linter;
        let mut cmd = Command::new(linter);
        cmd.arg("check");

        for arg in &self.ruff_config.args {
            cmd.arg(arg);
        }

        cmd.arg(py_file);
        cmd.current_dir(&self.project_root);
        log_command(&cmd);

        let output = cmd
            .output()
            .context(format!("Failed to run {}", linter))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            return Err(anyhow::anyhow!(
                "{} linting failed:\n{}{}",
                linter,
                stdout,
                stderr
            ));
        }

        write_stub(stub_path, "linted")
    }

    /// Run the configured linter on multiple files in a single invocation
    fn lint_files_batch(&self, py_files: &[&Path]) -> Result<()> {
        let linter = &self.ruff_config.linter;
        let mut cmd = Command::new(linter);
        cmd.arg("check");

        for arg in &self.ruff_config.args {
            cmd.arg(arg);
        }

        for file in py_files {
            cmd.arg(file);
        }
        cmd.current_dir(&self.project_root);
        log_command(&cmd);

        let output = cmd
            .output()
            .context(format!("Failed to run {}", linter))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            return Err(anyhow::anyhow!(
                "{} batch linting failed:\n{}{}",
                linter,
                stdout,
                stderr
            ));
        }

        Ok(())
    }
}

impl ProductDiscovery for RuffProcessor {
    fn description(&self) -> &str {
        "Lint Python files with ruff"
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        !file_index.scan(&self.project_root, &self.ruff_config.scan, true).is_empty()
    }

    fn required_tools(&self) -> Vec<String> {
        vec![self.ruff_config.linter.clone()]
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex) -> Result<()> {
        discover_stub_products(
            graph,
            &self.project_root,
            &self.stub_dir,
            &self.ruff_config.scan,
            file_index,
            &self.ruff_config.extra_inputs,
            &self.ruff_config,
            "ruff",
            "ruff",
            true,
        )
    }

    fn execute(&self, product: &Product) -> Result<()> {
        validate_stub_product(product, "Ruff")?;
        ensure_stub_dir(&self.stub_dir, "ruff")?;
        self.lint_file(&product.inputs[0], &product.outputs[0])
    }

    fn clean(&self, product: &Product) -> Result<()> {
        clean_outputs(product, "ruff")
    }

    fn supports_batch(&self) -> bool {
        true
    }

    fn execute_batch(&self, products: &[&Product]) -> Vec<Result<()>> {
        execute_lint_batch(
            products,
            "Ruff",
            &self.stub_dir,
            |files| self.lint_files_batch(files),
            |input, stub| self.lint_file(input, stub),
        )
    }
}
