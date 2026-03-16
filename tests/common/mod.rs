#![allow(dead_code)]

use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;
use serde::Deserialize;

/// Check if an external tool is available on PATH
pub fn tool_available(name: &str) -> bool {
    Command::new(name)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Helper to create a test project structure (tera processor only)
pub fn setup_test_project() -> TempDir {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    // Create directories
    fs::create_dir_all(temp_dir.path().join("templates.tera")).expect("Failed to create templates.tera dir");
    fs::create_dir_all(temp_dir.path().join("config")).expect("Failed to create config dir");

    // Only enable the tera processor so config/*.py files aren't picked up by linters
    fs::write(
        temp_dir.path().join("rsconstruct.toml"),
        "[processor]\nenabled = [\"tera\"]\n"
    ).expect("Failed to write rsconstruct.toml");

    temp_dir
}

/// Helper to run rsconstruct command in a directory
pub fn run_rsconstruct(dir: &Path, args: &[&str]) -> std::process::Output {
    let rsconstruct_path = env!("CARGO_BIN_EXE_rsconstruct");
    Command::new(rsconstruct_path)
        .current_dir(dir)
        .args(args)
        .output()
        .expect("Failed to execute rsconstruct")
}

/// Helper to run rsconstruct command with extra environment variables
pub fn run_rsconstruct_with_env(dir: &Path, args: &[&str], env_vars: &[(&str, &str)]) -> std::process::Output {
    let rsconstruct_path = env!("CARGO_BIN_EXE_rsconstruct");
    let mut cmd = Command::new(rsconstruct_path);
    cmd.current_dir(dir).args(args);
    for (key, val) in env_vars {
        cmd.env(key, val);
    }
    cmd.output().expect("Failed to execute rsconstruct")
}

/// Helper to set up a C project with the cc processor enabled
pub fn setup_cc_project(project_path: &Path) {
    fs::create_dir_all(project_path.join("src")).unwrap();
    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor]\nenabled = [\"cc_single_file\"]\n"
    ).unwrap();
}

// --- JSON output parsing for tests ---

/// Run rsconstruct with --json flag and return parsed build result
pub fn run_rsconstruct_json(dir: &Path, args: &[&str]) -> BuildResult {
    let mut full_args = vec!["--json"];
    full_args.extend(args);
    let output = run_rsconstruct(dir, &full_args);
    BuildResult::parse(&output)
}

/// Run rsconstruct with --json flag and extra environment variables
pub fn run_rsconstruct_json_with_env(dir: &Path, args: &[&str], env_vars: &[(&str, &str)]) -> BuildResult {
    let mut full_args = vec!["--json"];
    full_args.extend(args);
    let output = run_rsconstruct_with_env(dir, &full_args, env_vars);
    BuildResult::parse(&output)
}

/// JSON event from rsconstruct --json output
#[derive(Debug, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum BuildEvent {
    BuildStart {
        total_products: usize,
    },
    ProductComplete {
        product: String,
        processor: String,
        status: String,
        #[serde(default)]
        duration_ms: Option<u64>,
        #[serde(default)]
        error: Option<String>,
    },
    BuildSummary {
        total: usize,
        success: usize,
        failed: usize,
        skipped: usize,
        restored: usize,
        duration_ms: u64,
        #[serde(default)]
        errors: Vec<String>,
    },
}

/// Parsed build result from rsconstruct --json output
#[derive(Debug, Default)]
pub struct BuildResult {
    pub exit_success: bool,
    pub total_products: usize,
    pub success: usize,
    pub failed: usize,
    pub skipped: usize,
    pub restored: usize,
    pub duration_ms: u64,
    pub errors: Vec<String>,
    pub products: Vec<ProductResult>,
}

/// Individual product result
#[derive(Debug, Clone)]
pub struct ProductResult {
    pub product: String,
    pub processor: String,
    pub status: String,
    pub duration_ms: Option<u64>,
    pub error: Option<String>,
}

impl BuildResult {
    /// Parse rsconstruct --json output into structured BuildResult
    pub fn parse(output: &std::process::Output) -> Self {
        let mut result = BuildResult {
            exit_success: output.status.success(),
            ..Default::default()
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(event) = serde_json::from_str::<BuildEvent>(line) {
                match event {
                    BuildEvent::BuildStart { total_products } => {
                        result.total_products = total_products;
                    }
                    BuildEvent::ProductComplete { product, processor, status, duration_ms, error } => {
                        result.products.push(ProductResult {
                            product,
                            processor,
                            status,
                            duration_ms,
                            error,
                        });
                    }
                    BuildEvent::BuildSummary { total: _, success, failed, skipped, restored, duration_ms, errors } => {
                        result.success = success;
                        result.failed = failed;
                        result.skipped = skipped;
                        result.restored = restored;
                        result.duration_ms = duration_ms;
                        result.errors = errors;
                    }
                }
            }
        }
        result
    }

    /// Count products with a specific status
    pub fn count_status(&self, status: &str) -> usize {
        self.products.iter().filter(|p| p.status == status).count()
    }

    /// Check if a product with given name was processed with given status
    pub fn has_product(&self, name: &str, status: &str) -> bool {
        self.products.iter().any(|p| p.product.contains(name) && p.status == status)
    }

    /// Get all products with a specific status
    pub fn products_with_status(&self, status: &str) -> Vec<&ProductResult> {
        self.products.iter().filter(|p| p.status == status).collect()
    }
}
