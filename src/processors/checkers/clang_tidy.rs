use anyhow::Result;
use std::path::PathBuf;
use std::process::Command;

use crate::config::ClangTidyConfig;
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProductDiscovery, discover_checker_products, scan_root_valid, run_command, check_command_output};

pub struct ClangTidyProcessor {
    project_root: PathBuf,
    config: ClangTidyConfig,
}

impl ClangTidyProcessor {
    pub fn new(project_root: PathBuf, config: ClangTidyConfig) -> Self {
        Self {
            project_root,
            config,
        }
    }

    /// Check if clang-tidy analysis should be enabled
    fn should_check(&self) -> bool {
        scan_root_valid(&self.config.scan)
    }
}

impl ProductDiscovery for ClangTidyProcessor {
    fn description(&self) -> &str {
        "Run clang-tidy static analysis on C/C++ source files"
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        self.should_check() && !file_index.scan(&self.config.scan, true).is_empty()
    }

    fn required_tools(&self) -> Vec<String> {
        vec!["clang-tidy".to_string()]
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex) -> Result<()> {
        if !self.should_check() {
            return Ok(());
        }
        discover_checker_products(
            graph,
            &self.config.scan,
            file_index,
            &self.config.extra_inputs,
            &self.config,
            "clang_tidy",
        )
    }

    fn execute(&self, product: &Product) -> Result<()> {
        let mut cmd = Command::new("clang-tidy");
        for arg in &self.config.args {
            cmd.arg(arg);
        }
        cmd.arg(&product.inputs[0]);
        // Add -- to separate clang-tidy args from compiler args
        cmd.arg("--");
        for arg in &self.config.compiler_args {
            cmd.arg(arg);
        }
        cmd.current_dir(&self.project_root);

        let output = run_command(&mut cmd)?;
        check_command_output(&output, "clang-tidy")
    }

    fn config_json(&self) -> Option<String> {
        serde_json::to_string(&self.config).ok()
    }
}
