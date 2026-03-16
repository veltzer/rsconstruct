use std::fs;
use tempfile::TempDir;
use crate::common::{run_rsconstruct_with_env, tool_available};

#[test]
fn pyrefly_valid_python() {
    if !tool_available("pyrefly") {
        eprintln!("pyrefly not found, skipping test");
        return;
    }

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor]\nenabled = [\"pyrefly\"]\n",
    )
    .unwrap();

    fs::write(
        project_path.join("hello.py"),
        "def greet(name: str) -> str:\n    return f\"Hello, {name}!\"\n",
    )
    .unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build", "-v"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "Build should succeed with valid Python: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Processing:") || stdout.contains("Processing batch:"),
        "Should process pyrefly: {}",
        stdout
    );
}

#[test]
fn pyrefly_incremental_skip() {
    if !tool_available("pyrefly") {
        eprintln!("pyrefly not found, skipping test");
        return;
    }

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor]\nenabled = [\"pyrefly\"]\n",
    )
    .unwrap();

    fs::write(
        project_path.join("hello.py"),
        "def greet(name: str) -> str:\n    return f\"Hello, {name}!\"\n",
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
        stdout2.contains("[pyrefly] Skipping (unchanged):"),
        "Second build should skip: {}",
        stdout2
    );
}
