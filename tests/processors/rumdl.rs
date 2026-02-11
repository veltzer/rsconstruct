use std::fs;
use tempfile::TempDir;
use crate::common::{run_rsb_with_env, tool_available};

#[test]
fn rumdl_valid_markdown() {
    if !tool_available("rumdl") {
        eprintln!("rumdl not found, skipping test");
        return;
    }

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsb.toml"),
        "[processor]\nenabled = [\"rumdl\"]\n",
    )
    .unwrap();

    fs::write(
        project_path.join("readme.md"),
        "# Hello\n\nThis is a valid markdown file.\n",
    )
    .unwrap();

    let output = run_rsb_with_env(project_path, &["build", "-v"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "Build should succeed with valid markdown: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Processing:") || stdout.contains("Processing batch:"),
        "Should process rumdl: {}",
        stdout
    );
}

#[test]
fn rumdl_incremental_skip() {
    if !tool_available("rumdl") {
        eprintln!("rumdl not found, skipping test");
        return;
    }

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsb.toml"),
        "[processor]\nenabled = [\"rumdl\"]\n",
    )
    .unwrap();

    fs::write(
        project_path.join("readme.md"),
        "# Hello\n\nThis is a valid markdown file.\n",
    )
    .unwrap();

    // First build
    let output1 = run_rsb_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output1.status.success());

    // Second build should skip
    let output2 = run_rsb_with_env(project_path, &["build", "--verbose"], &[("NO_COLOR", "1")]);
    assert!(output2.status.success());
    let stdout2 = String::from_utf8_lossy(&output2.stdout);
    assert!(
        stdout2.contains("[rumdl] Skipping (unchanged):"),
        "Second build should skip: {}",
        stdout2
    );
}
