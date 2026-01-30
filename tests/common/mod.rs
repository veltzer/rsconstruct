#![allow(dead_code)]

use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

/// Helper to create a test project structure (template processor only)
pub fn setup_test_project() -> TempDir {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    // Create directories
    fs::create_dir_all(temp_dir.path().join("templates")).expect("Failed to create templates dir");
    fs::create_dir_all(temp_dir.path().join("config")).expect("Failed to create config dir");

    // Only enable the template processor so config/*.py files aren't picked up by linters
    fs::write(
        temp_dir.path().join("rsb.toml"),
        "[processor]\nenabled = [\"template\"]\n"
    ).expect("Failed to write rsb.toml");

    temp_dir
}

/// Helper to run rsb command in a directory
pub fn run_rsb(dir: &Path, args: &[&str]) -> std::process::Output {
    let rsb_path = env!("CARGO_BIN_EXE_rsb");
    Command::new(rsb_path)
        .current_dir(dir)
        .args(args)
        .output()
        .expect("Failed to execute rsb")
}

/// Helper to run rsb command with extra environment variables
pub fn run_rsb_with_env(dir: &Path, args: &[&str], env_vars: &[(&str, &str)]) -> std::process::Output {
    let rsb_path = env!("CARGO_BIN_EXE_rsb");
    let mut cmd = Command::new(rsb_path);
    cmd.current_dir(dir).args(args);
    for (key, val) in env_vars {
        cmd.env(key, val);
    }
    cmd.output().expect("Failed to execute rsb")
}

/// Helper to set up a C project with the cc processor enabled
pub fn setup_cc_project(project_path: &Path) {
    fs::create_dir_all(project_path.join("src")).unwrap();
    fs::write(
        project_path.join("rsb.toml"),
        "[processor]\nenabled = [\"cc_single_file\"]\n"
    ).unwrap();
}
