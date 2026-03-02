use std::fs;
use tempfile::TempDir;
use crate::common::run_rsb_with_env;

#[test]
fn script_check_valid_file() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    // script_check is disabled by default, so we must explicitly enable and configure it
    fs::write(
        project_path.join("rsb.toml"),
        concat!(
            "[processor]\n",
            "enabled = [\"script_check\"]\n",
            "\n",
            "[processor.script_check]\n",
            "enabled = true\n",
            "checker = \"true\"\n",
            "extensions = [\".txt\"]\n",
        ),
    )
    .unwrap();

    fs::write(
        project_path.join("test.txt"),
        "hello world\n",
    )
    .unwrap();

    let output = run_rsb_with_env(project_path, &["build", "-v"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "Build should succeed with script_check using 'true': stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Processing:"),
        "Should process script_check: {}",
        stdout
    );
}

#[test]
fn script_check_incremental_skip() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsb.toml"),
        concat!(
            "[processor]\n",
            "enabled = [\"script_check\"]\n",
            "\n",
            "[processor.script_check]\n",
            "enabled = true\n",
            "checker = \"true\"\n",
            "extensions = [\".txt\"]\n",
        ),
    )
    .unwrap();

    fs::write(
        project_path.join("test.txt"),
        "hello world\n",
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
        stdout2.contains("[script_check] Skipping (unchanged):"),
        "Second build should skip: {}",
        stdout2
    );
}

#[test]
fn script_check_no_project_discovered() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    // Without configuring extensions or checker, script_check should discover nothing
    fs::write(
        project_path.join("rsb.toml"),
        "[processor]\nenabled = [\"script_check\"]\n",
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
