use std::fs;
use tempfile::TempDir;
use crate::common::{run_rsbuild_with_env, tool_available};

#[test]
fn pandoc_valid_file() {
    if !tool_available("pandoc") {
        eprintln!("pandoc not found, skipping test");
        return;
    }

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    // Use HTML format to avoid requiring LaTeX for PDF
    fs::write(
        project_path.join("rsbuild.toml"),
        "[processor]\nenabled = [\"pandoc\"]\n\n[processor.pandoc]\nformats = [\"html\"]\nscan_dir = \"\"\n",
    )
    .unwrap();

    fs::write(
        project_path.join("doc.md"),
        "# Hello World\n\nThis is a test document.\n",
    )
    .unwrap();

    let output = run_rsbuild_with_env(project_path, &["build", "-v"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "Build should succeed with valid markdown for pandoc: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Processing:"),
        "Should process pandoc: {}",
        stdout
    );
}

#[test]
fn pandoc_incremental_skip() {
    if !tool_available("pandoc") {
        eprintln!("pandoc not found, skipping test");
        return;
    }

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsbuild.toml"),
        "[processor]\nenabled = [\"pandoc\"]\n\n[processor.pandoc]\nformats = [\"html\"]\nscan_dir = \"\"\n",
    )
    .unwrap();

    fs::write(
        project_path.join("doc.md"),
        "# Hello World\n\nThis is a test.\n",
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
        stdout2.contains("[pandoc] Skipping (unchanged):"),
        "Second build should skip: {}",
        stdout2
    );
}

#[test]
fn pandoc_no_project_discovered() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsbuild.toml"),
        "[processor]\nenabled = [\"pandoc\"]\n",
    )
    .unwrap();

    let output = run_rsbuild_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("0 products"),
        "Should discover 0 products: {}",
        stdout
    );
}
