use std::fs;
use tempfile::TempDir;
use crate::common::{run_rsbuild_with_env, tool_available};

#[test]
fn jq_valid_json() {
    if !tool_available("jq") {
        eprintln!("jq not found, skipping test");
        return;
    }

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsbuild.toml"),
        "[processor]\nenabled = [\"jq\"]\n",
    )
    .unwrap();

    fs::write(
        project_path.join("test.json"),
        "{\"name\": \"test\", \"value\": 42}\n",
    )
    .unwrap();

    let output = run_rsbuild_with_env(project_path, &["build", "-v"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "Build should succeed with valid JSON: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Processing:") || stdout.contains("Processing batch:"),
        "Should process jq: {}",
        stdout
    );
}

#[test]
fn jq_incremental_skip() {
    if !tool_available("jq") {
        eprintln!("jq not found, skipping test");
        return;
    }

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsbuild.toml"),
        "[processor]\nenabled = [\"jq\"]\n",
    )
    .unwrap();

    fs::write(
        project_path.join("test.json"),
        "{\"name\": \"test\", \"value\": 42}\n",
    )
    .unwrap();

    // First build
    let output1 = run_rsbuild_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output1.status.success());

    // Second build should skip
    let output2 = run_rsbuild_with_env(project_path, &["build", "--verbose"], &[("NO_COLOR", "1")]);
    assert!(output2.status.success());
    let stdout2 = String::from_utf8_lossy(&output2.stdout);
    assert!(
        stdout2.contains("[jq] Skipping (unchanged):"),
        "Second build should skip: {}",
        stdout2
    );
}
