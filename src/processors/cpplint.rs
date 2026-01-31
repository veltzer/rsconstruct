use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use crate::config::CpplintConfig;
use crate::graph::{BuildGraph, Product};
use crate::ignore::IgnoreRules;
use super::{ProductDiscovery, discover_stub_products, scan_files, scan_root, validate_stub_product, ensure_stub_dir, write_stub, clean_outputs};

const CPPLINT_STUB_DIR: &str = "out/cpplint";

pub struct Cpplinter {
    project_root: PathBuf,
    cpplint_config: CpplintConfig,
    stub_dir: PathBuf,
    ignore_rules: Arc<IgnoreRules>,
}

impl Cpplinter {
    pub fn new(project_root: PathBuf, cpplint_config: CpplintConfig, ignore_rules: Arc<IgnoreRules>) -> Self {
        let stub_dir = project_root.join(CPPLINT_STUB_DIR);
        Self {
            project_root,
            cpplint_config,
            stub_dir,
            ignore_rules,
        }
    }

    /// Check if C/C++ linting should be enabled
    fn should_lint(&self) -> bool {
        scan_root(&self.project_root, &self.cpplint_config.scan).exists()
    }

    /// Run checker on a single file and create stub
    fn check_file(&self, source_file: &Path, stub_path: &Path) -> Result<()> {
        let mut cmd = Command::new(&self.cpplint_config.checker);

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

        write_stub(stub_path, "checked")
    }
}

impl ProductDiscovery for Cpplinter {
    fn auto_detect(&self) -> bool {
        self.should_lint() && !scan_files(&self.project_root, &self.cpplint_config.scan, &self.ignore_rules, true).is_empty()
    }

    fn discover(&self, graph: &mut BuildGraph) -> Result<()> {
        if !self.should_lint() {
            return Ok(());
        }
        discover_stub_products(
            graph,
            &self.project_root,
            &self.stub_dir,
            &self.cpplint_config.scan,
            &self.ignore_rules,
            &self.cpplint_config.extra_inputs,
            &self.cpplint_config,
            "cpplint",
            "cpplint",
            true,
        )
    }

    fn execute(&self, product: &Product) -> Result<()> {
        validate_stub_product(product, "Cpplint")?;
        ensure_stub_dir(&self.stub_dir, "cpplint")?;
        self.check_file(&product.inputs[0], &product.outputs[0])
    }

    fn clean(&self, product: &Product) -> Result<()> {
        clean_outputs(product, "cpplint")
    }
}
