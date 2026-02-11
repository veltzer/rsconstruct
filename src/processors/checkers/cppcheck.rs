use anyhow::Result;
use std::path::PathBuf;
use std::process::Command;

use crate::config::CppcheckConfig;
use crate::graph::Product;
use crate::processors::{scan_root_valid, run_command, check_command_output};

pub struct CppcheckProcessor {
    project_root: PathBuf,
    config: CppcheckConfig,
}

impl CppcheckProcessor {
    pub fn new(project_root: PathBuf, config: CppcheckConfig) -> Self {
        Self {
            project_root,
            config,
        }
    }

    fn should_process(&self) -> bool {
        scan_root_valid(&self.config.scan)
    }

    fn execute_product(&self, product: &Product) -> Result<()> {
        let mut cmd = Command::new("cppcheck");
        for arg in &self.config.args {
            cmd.arg(arg);
        }
        cmd.arg(&product.inputs[0]);
        cmd.current_dir(&self.project_root);

        let output = run_command(&mut cmd)?;
        check_command_output(&output, "cppcheck")
    }
}

impl_checker!(CppcheckProcessor,
    config: config,
    description: "Run cppcheck static analysis on C/C++ source files",
    name: "cppcheck",
    execute: execute_product,
    guard: should_process,
    tools: ["cppcheck".to_string()],
    config_json: true,
);
