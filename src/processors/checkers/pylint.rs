use anyhow::Result;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::PylintConfig;
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProductDiscovery, discover_checker_products, run_command, check_command_output, execute_checker_batch};

pub struct PylintProcessor {
    project_root: PathBuf,
    pylint_config: PylintConfig,
}

impl PylintProcessor {
    pub fn new(project_root: PathBuf, pylint_config: PylintConfig) -> Self {
        Self {
            project_root,
            pylint_config,
        }
    }

    /// Run pylint on a single file
    fn lint_file(&self, py_file: &Path) -> Result<()> {
        let mut cmd = Command::new("pylint");

        for arg in &self.pylint_config.args {
            cmd.arg(arg);
        }

        cmd.arg(py_file);
        cmd.current_dir(&self.project_root);

        let output = run_command(&mut cmd)?;
        check_command_output(&output, "Pylint")
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

        let output = run_command(&mut cmd)?;
        check_command_output(&output, "Pylint batch")
    }
}

impl ProductDiscovery for PylintProcessor {
    fn description(&self) -> &str {
        "Lint Python files with pylint"
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        !file_index.scan(&self.pylint_config.scan, true).is_empty()
    }

    fn required_tools(&self) -> Vec<String> {
        vec!["pylint".to_string()]
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex) -> Result<()> {
        let mut extra_inputs = self.pylint_config.extra_inputs.clone();
        // pylint implicitly reads .pylintrc from the project root if present
        let pylintrc = Path::new(".pylintrc");
        if pylintrc.exists() {
            extra_inputs.push(".pylintrc".to_string());
        }
        discover_checker_products(
            graph,
            &self.pylint_config.scan,
            file_index,
            &extra_inputs,
            &self.pylint_config,
            "pylint",
        )
    }

    fn execute(&self, product: &Product) -> Result<()> {
        self.lint_file(&product.inputs[0])
    }

    fn supports_batch(&self) -> bool {
        true
    }

    fn execute_batch(&self, products: &[&Product]) -> Vec<Result<()>> {
        execute_checker_batch(
            products,
            |files| self.lint_files_batch(files),
            |input| self.lint_file(input),
        )
    }
}
