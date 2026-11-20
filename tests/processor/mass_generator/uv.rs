use crate::common::{require_tool, run_rsconstruct_with_env};
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

/// A project with no dependencies, locked by uv itself so the lock is in
/// the format of the uv under test.
fn project() -> TempDir {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let path = temp_dir.path();
    fs::write(
        path.join("rsconstruct.toml"),
        "[processor.mass_generator.uv]\nsrc_dirs = [\".\"]\n",
    )
    .unwrap();
    write_pyproject(path, "0.1.0");
    let locked = Command::new("uv")
        .arg("lock")
        .current_dir(path)
        .output()
        .expect("uv runs");
    assert!(
        locked.status.success(),
        "uv lock failed: {}",
        String::from_utf8_lossy(&locked.stderr)
    );
    temp_dir
}

fn write_pyproject(path: &Path, version: &str) {
    fs::write(
        path.join("pyproject.toml"),
        format!(
            "[project]\nname = \"demo\"\nversion = \"{version}\"\nrequires-python = \">=3.8\"\ndependencies = []\n\n[tool.uv]\npackage = false\n"
        ),
    )
    .unwrap();
}

#[test]
fn uv_syncs_and_skips_when_unchanged() {
    require_tool("uv");

    let temp_dir = project();
    let project_path = temp_dir.path();

    let output = run_rsconstruct_with_env(project_path, &["build", "-v"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "Build should succeed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(project_path.join(".venv/pyvenv.cfg").exists());
    assert!(
        project_path
            .join("out/processor.mass_generator.uv/root.stamp")
            .exists()
    );

    let again =
        run_rsconstruct_with_env(project_path, &["build", "--verbose"], &[("NO_COLOR", "1")]);
    assert!(again.status.success());
    let stdout = String::from_utf8_lossy(&again.stdout);
    assert!(
        stdout.contains("[processor.mass_generator.uv] Skipping (unchanged):"),
        "Second build should skip: {stdout}"
    );
}

/// `--locked` is a default argument: a pyproject.toml that moved on from
/// its uv.lock fails the build instead of being re-resolved by it.
#[test]
fn uv_fails_on_a_stale_lock() {
    require_tool("uv");

    let temp_dir = project();
    let project_path = temp_dir.path();
    fs::write(
        project_path.join("pyproject.toml"),
        "[project]\nname = \"demo\"\nversion = \"0.1.0\"\nrequires-python = \">=3.8\"\ndependencies = [\"iniconfig\"]\n\n[tool.uv]\npackage = false\n",
    )
    .unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(!output.status.success(), "a stale lock must fail the build");
}

#[test]
fn uv_no_project_discovered() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();
    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.mass_generator.uv]\nsrc_dirs = [\".\"]\n",
    )
    .unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("0 products"), "{stdout}");
}
