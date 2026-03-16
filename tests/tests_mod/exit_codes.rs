use std::fs;
use tempfile::TempDir;
use crate::common::{run_rsconstruct, run_rsconstruct_with_env, setup_test_project};

#[test]
fn missing_config_returns_config_error() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    // No rsconstruct.toml in directory
    let output = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(!output.status.success());

    let exit_code = output.status.code().unwrap();
    assert_eq!(exit_code, 2, "Expected exit code 2 (CONFIG_ERROR), got {}", exit_code);

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("CONFIG_ERROR"), "Expected CONFIG_ERROR in stderr, got: {}", stderr);
}

#[test]
fn init_already_exists_returns_config_error() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    // Create rsconstruct.toml first
    fs::write(project_path.join("rsconstruct.toml"), "# existing").unwrap();

    let output = run_rsconstruct_with_env(project_path, &["init"], &[("NO_COLOR", "1")]);
    assert!(!output.status.success());

    let exit_code = output.status.code().unwrap();
    assert_eq!(exit_code, 2, "Expected exit code 2 (CONFIG_ERROR), got {}", exit_code);

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("CONFIG_ERROR"), "Expected CONFIG_ERROR in stderr, got: {}", stderr);
}

#[test]
fn success_returns_zero() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    let output = run_rsconstruct(project_path, &["init"]);
    assert!(output.status.success());

    let exit_code = output.status.code().unwrap();
    assert_eq!(exit_code, 0, "Expected exit code 0 (SUCCESS), got {}", exit_code);
}

#[test]
fn unknown_processor_returns_config_error() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    let output = run_rsconstruct_with_env(project_path, &["build", "-p", "nonexistent"], &[("NO_COLOR", "1")]);
    assert!(!output.status.success());

    let exit_code = output.status.code().unwrap();
    assert_eq!(exit_code, 2, "Expected exit code 2 (CONFIG_ERROR), got {}", exit_code);

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("CONFIG_ERROR"), "Expected CONFIG_ERROR in stderr, got: {}", stderr);
}
