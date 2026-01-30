mod common;

use std::fs;
use tempfile::TempDir;
use common::run_rsb;

#[test]
fn init_creates_project() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    let output = run_rsb(project_path, &["init"]);
    assert!(output.status.success(), "rsb init failed: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Check files/dirs were created
    assert!(project_path.join("rsb.toml").exists(), "rsb.toml should be created");
    assert!(project_path.join("templates").exists(), "templates/ should be created");
    assert!(project_path.join("config").exists(), "config/ should be created");

    // Verify rsb.toml has content
    let toml_content = fs::read_to_string(project_path.join("rsb.toml")).unwrap();
    assert!(toml_content.contains("[build]"), "rsb.toml should contain [build] section");
    assert!(toml_content.contains("[processor]"), "rsb.toml should contain [processor] section");

    assert!(stdout.contains("Created"), "Output should mention Created");
}

#[test]
fn init_fails_if_exists() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    // Create rsb.toml first
    fs::write(project_path.join("rsb.toml"), "# existing").unwrap();

    let output = run_rsb(project_path, &["init"]);
    assert!(!output.status.success(), "rsb init should fail if rsb.toml exists");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("already exists"), "Error should mention 'already exists': {}", stderr);
}

#[test]
fn init_preserves_existing_dirs() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    // Create templates dir with a file
    fs::create_dir_all(project_path.join("templates")).unwrap();
    fs::write(project_path.join("templates/existing.txt"), "do not delete").unwrap();

    let output = run_rsb(project_path, &["init"]);
    assert!(output.status.success());

    // Existing file should still be there
    assert!(project_path.join("templates/existing.txt").exists(),
        "Existing files in templates/ should be preserved");
    let content = fs::read_to_string(project_path.join("templates/existing.txt")).unwrap();
    assert_eq!(content, "do not delete");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("already exists"), "Should mention that directory already exists");
}
