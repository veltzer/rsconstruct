use std::fs;
use tempfile::TempDir;
use crate::common::run_rsconstruct;

#[test]
fn init_creates_project() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    let output = run_rsconstruct(project_path, &["init"]);
    assert!(output.status.success(), "rsconstruct init failed: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Check files were created
    assert!(project_path.join("rsconstruct.toml").exists(), "rsconstruct.toml should be created");
    assert!(project_path.join(".rsconstructignore").exists(), ".rsconstructignore should be created");

    // Verify rsconstruct.toml has content
    let toml_content = fs::read_to_string(project_path.join("rsconstruct.toml")).unwrap();
    assert!(toml_content.contains("[build]"), "rsconstruct.toml should contain [build] section");
    assert!(toml_content.contains("[processor]"), "rsconstruct.toml should contain [processor] section");

    // Verify .rsconstructignore has content
    let rsconstructignore_content = fs::read_to_string(project_path.join(".rsconstructignore")).unwrap();
    assert!(rsconstructignore_content.contains(".gitignore syntax"), ".rsconstructignore should reference gitignore syntax");

    assert!(stdout.contains("Created"), "Output should mention Created");
}

#[test]
fn init_fails_if_exists() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    // Create rsconstruct.toml first
    fs::write(project_path.join("rsconstruct.toml"), "# existing").unwrap();

    let output = run_rsconstruct(project_path, &["init"]);
    assert!(!output.status.success(), "rsconstruct init should fail if rsconstruct.toml exists");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("already exists"), "Error should mention 'already exists': {}", stderr);
}

#[test]
fn init_ignores_existing_dirs() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    // Create tera.templates dir with a file
    fs::create_dir_all(project_path.join("tera.templates")).unwrap();
    fs::write(project_path.join("tera.templates/existing.txt"), "do not delete").unwrap();

    let output = run_rsconstruct(project_path, &["init"]);
    assert!(output.status.success());

    // Existing file should still be there
    assert!(project_path.join("tera.templates/existing.txt").exists(),
        "Existing files in tera.templates/ should be preserved");
    let content = fs::read_to_string(project_path.join("tera.templates/existing.txt")).unwrap();
    assert_eq!(content, "do not delete");
}

#[test]
fn init_preserves_existing_rsconstructignore() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    // Create .rsconstructignore with custom content before init
    let custom_content = "# my custom ignore rules\n*.tmp\n";
    fs::write(project_path.join(".rsconstructignore"), custom_content).unwrap();

    let output = run_rsconstruct(project_path, &["init"]);
    assert!(output.status.success(), "rsconstruct init failed: {}", String::from_utf8_lossy(&output.stderr));

    // Verify .rsconstructignore was not overwritten
    let content = fs::read_to_string(project_path.join(".rsconstructignore")).unwrap();
    assert_eq!(content, custom_content, ".rsconstructignore should not be overwritten");
}
