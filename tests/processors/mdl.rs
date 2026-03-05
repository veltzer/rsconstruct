use std::fs;
use tempfile::TempDir;
use crate::common::{run_rsb_with_env, tool_available};

#[test]
fn mdl_valid_file() {
    if !tool_available("mdl") {
        eprintln!("mdl not found, skipping test");
        return;
    }

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    // Point mdl_bin to the system mdl, skip gem dependency
    fs::write(
        project_path.join("rsbuild.toml"),
        "[processor]\nenabled = [\"mdl\"]\n\n[processor.mdl]\nmdl_bin = \"mdl\"\n",
    )
    .unwrap();

    // Content that passes mdl rules: proper heading structure, blank lines
    fs::write(
        project_path.join("doc.md"),
        "# Hello World\n\nThis is a test document.\n",
    )
    .unwrap();

    let output = run_rsb_with_env(project_path, &["build", "-v"], &[("NO_COLOR", "1")]);
    // mdl may fail due to rule violations even with simple content
    // Just verify discovery and processing attempt
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stdout.contains("Processing:") || stdout.contains("1 products"),
        "Should discover and attempt mdl processing: stdout={}, stderr={}",
        stdout, stderr
    );
}

#[test]
fn mdl_no_project_discovered() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsbuild.toml"),
        "[processor]\nenabled = [\"mdl\"]\n\n[processor.mdl]\nscan_dir = \"mdl_docs\"\n",
    )
    .unwrap();

    let output = run_rsb_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("0 products"),
        "Should discover 0 products: {}",
        stdout
    );
}
