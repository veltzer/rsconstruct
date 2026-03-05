use std::fs;
use crate::common::{setup_test_project, run_rsbuild_with_env, run_rsbuild};

#[test]
fn explain_first_build() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    fs::create_dir_all(project_path.join("sleep")).unwrap();
    fs::write(project_path.join("sleep/explain_first.sleep"), "0.01").unwrap();
    fs::write(
        project_path.join("rsbuild.toml"),
        "[processor]\nenabled = [\"sleep\"]\n"
    ).unwrap();

    let output = run_rsbuild_with_env(project_path, &["build", "--explain"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(), "build failed: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("BUILD"), "Expected BUILD in explain output, got: {}", stdout);
    assert!(stdout.contains("no cache entry"), "Expected 'no cache entry' reason, got: {}", stdout);
}

#[test]
fn explain_incremental_skip() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    fs::create_dir_all(project_path.join("sleep")).unwrap();
    fs::write(project_path.join("sleep/explain_skip.sleep"), "0.01").unwrap();
    fs::write(
        project_path.join("rsbuild.toml"),
        "[processor]\nenabled = [\"sleep\"]\n"
    ).unwrap();

    // First build
    let output = run_rsbuild(project_path, &["build"]);
    assert!(output.status.success());

    // Second build with explain
    let output = run_rsbuild_with_env(project_path, &["build", "--explain"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("SKIP"), "Expected SKIP in explain output, got: {}", stdout);
    assert!(stdout.contains("inputs unchanged"), "Expected 'inputs unchanged' reason, got: {}", stdout);
}

#[test]
fn explain_input_change() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    fs::create_dir_all(project_path.join("sleep")).unwrap();
    fs::write(project_path.join("sleep/explain_change.sleep"), "0.01").unwrap();
    fs::write(
        project_path.join("rsbuild.toml"),
        "[processor]\nenabled = [\"sleep\"]\n"
    ).unwrap();

    // First build
    let output = run_rsbuild(project_path, &["build"]);
    assert!(output.status.success());

    // Modify the input
    fs::write(project_path.join("sleep/explain_change.sleep"), "0.02").unwrap();

    // Second build with explain
    let output = run_rsbuild_with_env(project_path, &["build", "--explain"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("BUILD"), "Expected BUILD in explain output, got: {}", stdout);
    assert!(stdout.contains("inputs changed"), "Expected 'inputs changed' reason, got: {}", stdout);
}

#[test]
fn explain_force() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    fs::create_dir_all(project_path.join("sleep")).unwrap();
    fs::write(project_path.join("sleep/explain_force.sleep"), "0.01").unwrap();
    fs::write(
        project_path.join("rsbuild.toml"),
        "[processor]\nenabled = [\"sleep\"]\n"
    ).unwrap();

    // First build
    let output = run_rsbuild(project_path, &["build"]);
    assert!(output.status.success());

    // Force build with explain
    let output = run_rsbuild_with_env(project_path, &["build", "--force", "--explain"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("BUILD"), "Expected BUILD in explain output, got: {}", stdout);
    assert!(stdout.contains("forced"), "Expected 'forced' reason, got: {}", stdout);
}

#[test]
fn explain_after_clean() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Use tera processor which is a generator (produces output files)
    fs::write(
        project_path.join("templates.tera/explain_clean.txt.tera"),
        "hello"
    ).unwrap();

    // First build
    let output = run_rsbuild(project_path, &["build"]);
    assert!(output.status.success());

    // Clean outputs
    let output = run_rsbuild(project_path, &["clean"]);
    assert!(output.status.success());

    // Build with explain — should show RESTORE
    let output = run_rsbuild_with_env(project_path, &["build", "--explain"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("RESTORE"), "Expected RESTORE in explain output, got: {}", stdout);
    assert!(stdout.contains("output missing"), "Expected 'output missing' reason, got: {}", stdout);
}

#[test]
fn explain_dry_run() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    fs::create_dir_all(project_path.join("sleep")).unwrap();
    fs::write(project_path.join("sleep/explain_dry.sleep"), "0.01").unwrap();
    fs::write(
        project_path.join("rsbuild.toml"),
        "[processor]\nenabled = [\"sleep\"]\n"
    ).unwrap();

    // Dry run with explain on first build
    let output = run_rsbuild_with_env(project_path, &["build", "--dry-run", "--explain"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("BUILD"), "Expected BUILD in explain dry-run output, got: {}", stdout);
    assert!(stdout.contains("no cache entry"), "Expected 'no cache entry' reason in dry-run, got: {}", stdout);
}
