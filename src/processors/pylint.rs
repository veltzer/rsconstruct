use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::PylintConfig;
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use super::{ProductDiscovery, discover_stub_products, validate_stub_product, ensure_stub_dir, write_stub, clean_outputs, log_command, execute_lint_batch};

const PYLINT_STUB_DIR: &str = "out/pylint";

pub struct PylintProcessor {
    project_root: PathBuf,
    pylint_config: PylintConfig,
    stub_dir: PathBuf,
}

impl PylintProcessor {
    pub fn new(project_root: PathBuf, pylint_config: PylintConfig) -> Self {
        let stub_dir = project_root.join(PYLINT_STUB_DIR);
        Self {
            project_root,
            pylint_config,
            stub_dir,
        }
    }

    /// Run pylint on a single file and create stub
    fn lint_file(&self, py_file: &Path, stub_path: &Path) -> Result<()> {
        let mut cmd = Command::new("pylint");

        for arg in &self.pylint_config.args {
            cmd.arg(arg);
        }

        cmd.arg(py_file);
        cmd.current_dir(&self.project_root);
        log_command(&cmd);

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

        write_stub(stub_path, "linted")
    }

    /// Run pylint on multiple files in a single invocation
    fn lint_files_batch(&self, py_files: &[&Path]) -> Result<()> {
        let mut cmd = Command::new("pylint");

        for arg in &self.pylint_config.args {
            cmd.arg(arg);
        }

        for file in py_files {
            cmd.arg(file);
        }
        cmd.current_dir(&self.project_root);
        log_command(&cmd);

        let output = cmd
            .output()
            .context("Failed to run pylint")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            return Err(anyhow::anyhow!(
                "Pylint batch failed:\n{}{}",
                stdout,
                stderr
            ));
        }

        Ok(())
    }
}

impl ProductDiscovery for PylintProcessor {
    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        !file_index.scan(&self.project_root, &self.pylint_config.scan, true).is_empty()
    }

    fn required_tools(&self) -> Vec<String> {
        vec!["pylint".to_string()]
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex) -> Result<()> {
        let mut extra_inputs = self.pylint_config.extra_inputs.clone();
        // pylint implicitly reads .pylintrc from the project root if present
        let pylintrc = self.project_root.join(".pylintrc");
        if pylintrc.exists() {
            extra_inputs.push(".pylintrc".to_string());
        }
        discover_stub_products(
            graph,
            &self.project_root,
            &self.stub_dir,
            &self.pylint_config.scan,
            file_index,
            &extra_inputs,
            &self.pylint_config,
            "pylint",
            "pylint",
            true,
        )
    }

    fn execute(&self, product: &Product) -> Result<()> {
        validate_stub_product(product, "Pylint")?;
        ensure_stub_dir(&self.stub_dir, "pylint")?;
        self.lint_file(&product.inputs[0], &product.outputs[0])
    }

    fn clean(&self, product: &Product) -> Result<()> {
        clean_outputs(product, "pylint")
    }

    fn supports_batch(&self) -> bool {
        true
    }

    fn execute_batch(&self, products: &[&Product]) -> Vec<Result<()>> {
        execute_lint_batch(
            products,
            "Pylint",
            &self.stub_dir,
            |files| self.lint_files_batch(files),
            |input, stub| self.lint_file(input, stub),
        )
    }
}
