use std::fs;
use tempfile::TempDir;
use crate::common::run_rsconstruct_with_env;

#[test]
fn ascii_check_valid_file() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.ascii_check]\n",
    )
    .unwrap();

    fs::write(
        project_path.join("hello.md"),
        "# Hello World\n\nThis is plain ASCII text.\n",
    )
    .unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build", "-v"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "Build should succeed with ASCII-only file: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Processing:"),
        "Should process ascii_check: {}",
        stdout
    );
}

#[test]
fn ascii_check_incremental_skip() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.ascii_check]\n",
    )
    .unwrap();

    fs::write(
        project_path.join("hello.md"),
        "# Hello\n\nPlain ASCII.\n",
    )
    .unwrap();

    // First build
    let output1 = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output1.status.success());

    // Second build should skip
    let output2 = run_rsconstruct_with_env(project_path, &["build", "--verbose"], &[("NO_COLOR", "1")]);
    assert!(output2.status.success());
    let stdout2 = String::from_utf8_lossy(&output2.stdout);
    assert!(
        stdout2.contains("[ascii_check] Skipping (unchanged):"),
        "Second build should skip: {}",
        stdout2
    );
}

#[test]
fn ascii_check_non_ascii_fails() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.ascii_check]\n",
    )
    .unwrap();

    // Write a file with non-ASCII bytes (UTF-8 encoded 'e' with acute)
    fs::write(
        project_path.join("bad.md"),
        b"# Hello \xc3\xa9\n" as &[u8],
    )
    .unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(
        !output.status.success(),
        "Build should fail with non-ASCII characters"
    );
}
