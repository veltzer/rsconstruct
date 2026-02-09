use std::fs;
use tempfile::TempDir;
use crate::common::{run_rsb_with_env, tool_available};

#[test]
fn cargo_valid_project() {
    if !tool_available("cargo") {
        eprintln!("cargo not found, skipping test");
        return;
    }

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsb.toml"),
        "[processor]\nenabled = [\"cargo\"]\n",
    )
    .unwrap();

    // Create a minimal Rust library project
    fs::create_dir_all(project_path.join("mylib/src")).unwrap();

    fs::write(
        project_path.join("mylib/Cargo.toml"),
        "[package]\nname = \"mylib\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();

    fs::write(
        project_path.join("mylib/src/lib.rs"),
        "pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n",
    )
    .unwrap();

    let output = run_rsb_with_env(project_path, &["build", "-v"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "Build should succeed with valid Cargo project: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Processing:"),
        "Should process cargo: {}",
        stdout
    );
}

#[test]
fn cargo_incremental_skip() {
    if !tool_available("cargo") {
        eprintln!("cargo not found, skipping test");
        return;
    }

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsb.toml"),
        "[processor]\nenabled = [\"cargo\"]\n",
    )
    .unwrap();

    fs::create_dir_all(project_path.join("mylib/src")).unwrap();

    fs::write(
        project_path.join("mylib/Cargo.toml"),
        "[package]\nname = \"mylib\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();

    fs::write(
        project_path.join("mylib/src/lib.rs"),
        "pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n",
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
        stdout2.contains("[cargo] Skipping (unchanged):"),
        "Second build should skip: {}",
        stdout2
    );
}
