use std::fs;
use tempfile::TempDir;
use crate::common::run_rsconstruct_with_env;

fn setup_black_check_project() -> TempDir {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    fs::write(
        temp_dir.path().join("rsconstruct.toml"),
        "[processor.black_check]\n",
    )
    .expect("Failed to write rsconstruct.toml");
    temp_dir
}

#[test]
fn black_check_valid_file() {
    let temp_dir = setup_black_check_project();
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("hello.py"),
        "def hello():\n    return \"world\"\n",
    )
    .unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build", "-v"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "Build should succeed with well-formatted Python file: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Processing:"),
        "Should process black_check: {}",
        stdout
    );
}

#[test]
fn black_check_incremental_skip() {
    let temp_dir = setup_black_check_project();
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("hello.py"),
        "def hello():\n    return \"world\"\n",
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
        stdout2.contains("[black_check] Skipping (unchanged):"),
        "Second build should skip: {}",
        stdout2
    );
}

#[test]
fn black_check_badly_formatted_fails() {
    let temp_dir = setup_black_check_project();
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("bad.py"),
        "def hello(  ):\n    x=1\n    return    \"world\"\n",
    )
    .unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(
        !output.status.success(),
        "Build should fail with badly formatted Python file"
    );
}
