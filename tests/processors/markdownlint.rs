use std::fs;
use tempfile::TempDir;
use crate::common::{run_rsconstruct_with_env, tool_available};

/// Check if markdownlint is available on PATH.
fn markdownlint_available() -> bool {
    tool_available("markdownlint")
}

#[test]
fn markdownlint_valid_file() {
    if !markdownlint_available() {
        eprintln!("markdownlint not found, skipping test");
        return;
    }

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    // Point markdownlint_bin to the system markdownlint, skip npm dependency
    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor]\nenabled = [\"markdownlint\"]\n\n[processor.markdownlint]\nmarkdownlint_bin = \"markdownlint\"\n",
    )
    .unwrap();

    fs::write(
        project_path.join("doc.md"),
        "# Hello World\n\nThis is a test.\n",
    )
    .unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build", "-v"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "Build should succeed with valid markdown: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Processing:"),
        "Should process markdownlint: {}",
        stdout
    );
}

#[test]
fn markdownlint_no_project_discovered() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor]\nenabled = [\"markdownlint\"]\n\n[processor.markdownlint]\nscan_dir = \"mdlint_docs\"\n",
    )
    .unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("0 products"),
        "Should discover 0 products: {}",
        stdout
    );
}
