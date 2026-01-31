use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use crate::config::PylintConfig;
use crate::graph::{BuildGraph, Product};
use crate::ignore::IgnoreRules;
use super::{ProductDiscovery, discover_stub_products, scan_files, validate_stub_product, ensure_stub_dir, write_stub, clean_outputs};

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

    /// Run pylint on a single file and create stub
    fn lint_file(&self, py_file: &Path, stub_path: &Path) -> Result<()> {
        let mut cmd = Command::new("pylint");

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

        write_stub(stub_path, "linted")
    }
}

impl ProductDiscovery for PylintProcessor {
    fn auto_detect(&self) -> bool {
        !scan_files(&self.project_root, &self.pylint_config.scan, &self.ignore_rules, true).is_empty()
    }

    fn required_tools(&self) -> Vec<String> {
        vec!["pylint".to_string()]
    }

    fn discover(&self, graph: &mut BuildGraph) -> Result<()> {
        discover_stub_products(
            graph,
            &self.project_root,
            &self.stub_dir,
            &self.pylint_config.scan,
            &self.ignore_rules,
            &self.pylint_config.extra_inputs,
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
}
