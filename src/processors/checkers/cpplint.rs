use anyhow::Result;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::CpplintConfig;
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProductDiscovery, discover_checker_products, scan_root, run_command, check_command_output};

pub struct CpplintProcessor {
    project_root: PathBuf,
    cpplint_config: CpplintConfig,
}

impl CpplintProcessor {
    pub fn new(project_root: PathBuf, cpplint_config: CpplintConfig) -> Self {
        Self {
            project_root,
            cpplint_config,
        }
    }

    /// Check if C/C++ linting should be enabled
    fn should_lint(&self) -> bool {
        scan_root(&self.project_root, &self.cpplint_config.scan).exists()
    }

    /// Run checker on a single file
    fn check_file(&self, source_file: &Path) -> Result<()> {
        let mut cmd = Command::new(&self.cpplint_config.checker);

        for arg in &self.cpplint_config.args {
            cmd.arg(arg);
        }

        cmd.arg(source_file);
        cmd.current_dir(&self.project_root);

        let output = run_command(&mut cmd)?;
        check_command_output(&output, "C/C++ checking")
    }
}

impl ProductDiscovery for CpplintProcessor {
    fn description(&self) -> &str {
        "Run static analysis on C/C++ source files"
    }

    fn processor_type(&self) -> crate::processors::ProcessorType {
        crate::processors::ProcessorType::Checker
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        self.should_lint() && !file_index.scan(&self.project_root, &self.cpplint_config.scan, true).is_empty()
    }

    fn required_tools(&self) -> Vec<String> {
        vec![self.cpplint_config.checker.clone()]
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex) -> Result<()> {
        if !self.should_lint() {
            return Ok(());
        }
        discover_checker_products(
            graph,
            &self.project_root,
            &self.cpplint_config.scan,
            file_index,
            &self.cpplint_config.extra_inputs,
            &self.cpplint_config,
            "cpplint",
        )
    }

    fn execute(&self, product: &Product) -> Result<()> {
        if product.inputs.is_empty() {
            anyhow::bail!("Cpplint product must have at least one input");
        }
        self.check_file(&product.inputs[0])
    }

    fn clean(&self, _product: &Product) -> Result<()> {
        // No output files to clean for checkers
        Ok(())
    }
}
