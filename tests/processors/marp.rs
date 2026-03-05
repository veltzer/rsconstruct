use std::fs;
use tempfile::TempDir;
use crate::common::{run_rsbuild_with_env, tool_available};

#[test]
fn marp_valid_file() {
    if !tool_available("marp") {
        eprintln!("marp not found, skipping test");
        return;
    }

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsbuild.toml"),
        "[processor]\nenabled = [\"marp\"]\n\n[processor.marp]\nformats = [\"html\"]\n",
    )
    .unwrap();

    fs::write(
        project_path.join("slides.md"),
        "---\nmarp: true\n---\n\n# Slide 1\n\nHello World\n\n---\n\n# Slide 2\n\nGoodbye\n",
    )
    .unwrap();

    let output = run_rsbuild_with_env(project_path, &["build", "-v"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "Build should succeed with valid Marp file: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Processing:"),
        "Should process marp: {}",
        stdout
    );
}

#[test]
fn marp_no_project_discovered() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsbuild.toml"),
        "[processor]\nenabled = [\"marp\"]\n\n[processor.marp]\nscan_dir = \"marp\"\n",
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
