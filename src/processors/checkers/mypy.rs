use anyhow::Result;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::MypyConfig;
use crate::graph::Product;
use crate::processors::{run_command, check_command_output};

pub struct MypyProcessor {
    project_root: PathBuf,
    config: MypyConfig,
}

impl MypyProcessor {
    pub fn new(project_root: PathBuf, config: MypyConfig) -> Self {
        Self {
            project_root,
            config,
        }
    }

    fn execute_product(&self, product: &Product) -> Result<()> {
        self.check_files(&[product.inputs[0].as_path()])
    }

    /// Return extra inputs for discover: mypy.ini if it exists
    fn mypy_ini_inputs(&self) -> Vec<String> {
        if Path::new("mypy.ini").exists() {
            vec!["mypy.ini".to_string()]
        } else {
            Vec::new()
        }
    }

    /// Run mypy on one or more files
    fn check_files(&self, py_files: &[&Path]) -> Result<()> {
        let mut cmd = Command::new(&self.config.checker);

        for arg in &self.config.args {
            cmd.arg(arg);
        }

        for file in py_files {
            cmd.arg(file);
        }
        cmd.current_dir(&self.project_root);

        let output = run_command(&mut cmd)?;
        check_command_output(&output, "mypy")
    }
}

impl_checker!(MypyProcessor,
    config: config,
    description: "Type-check Python files with mypy",
    name: "mypy",
    execute: execute_product,
    tool_field: checker,
    config_json: true,
    batch: check_files,
    extra_discover_inputs: mypy_ini_inputs,
);
