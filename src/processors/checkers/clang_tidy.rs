use anyhow::Result;
use std::path::PathBuf;
use std::process::Command;

use crate::config::ClangTidyConfig;
use crate::graph::Product;
use crate::processors::{scan_root_valid, run_command, check_command_output};

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

    fn should_process(&self) -> bool {
        scan_root_valid(&self.config.scan)
    }

    fn execute_product(&self, product: &Product) -> Result<()> {
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
}

impl_checker!(ClangTidyProcessor,
    config: config,
    description: "Run clang-tidy static analysis on C/C++ source files",
    name: "clang_tidy",
    execute: execute_product,
    guard: should_process,
    tools: ["clang-tidy".to_string()],
    config_json: true,
);
