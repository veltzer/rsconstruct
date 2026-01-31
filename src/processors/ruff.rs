use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use crate::config::RuffConfig;
use crate::graph::{BuildGraph, Product};
use crate::ignore::IgnoreRules;
use super::{ProductDiscovery, discover_stub_products, scan_files, validate_stub_product, ensure_stub_dir, write_stub, clean_outputs};

const RUFF_STUB_DIR: &str = "out/ruff";

pub struct RuffProcessor {
    project_root: PathBuf,
    ruff_config: RuffConfig,
    stub_dir: PathBuf,
    ignore_rules: Arc<IgnoreRules>,
}

impl RuffProcessor {
    pub fn new(project_root: PathBuf, ruff_config: RuffConfig, ignore_rules: Arc<IgnoreRules>) -> Self {
        let stub_dir = project_root.join(RUFF_STUB_DIR);
        Self {
            project_root,
            ruff_config,
            stub_dir,
            ignore_rules,
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
}

impl ProductDiscovery for RuffProcessor {
    fn auto_detect(&self) -> bool {
        !scan_files(&self.project_root, &self.ruff_config.scan, &self.ignore_rules, true).is_empty()
    }

    fn discover(&self, graph: &mut BuildGraph) -> Result<()> {
        discover_stub_products(
            graph,
            &self.project_root,
            &self.stub_dir,
            &self.ruff_config.scan,
            &self.ignore_rules,
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
}
