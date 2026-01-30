mod common;

use std::fs;
use std::process::Command;
use std::time::Duration;
use common::setup_test_project;

#[test]
fn watch_does_initial_build() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    fs::create_dir_all(project_path.join("sleep")).unwrap();
    fs::write(project_path.join("sleep/watch_init.sleep"), "0.01").unwrap();
    fs::write(
        project_path.join("rsb.toml"),
        "[processor]\nenabled = [\"sleep\"]\n"
    ).unwrap();

    let rsb_path = env!("CARGO_BIN_EXE_rsb");
    let mut child = Command::new(rsb_path)
        .current_dir(project_path)
        .args(["watch"])
        .env("NO_COLOR", "1")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("Failed to spawn rsb watch");

    // Wait for initial build to complete
    std::thread::sleep(Duration::from_secs(2));

    // Kill the watcher
    child.kill().expect("Failed to kill watcher");
    let output = child.wait_with_output().expect("Failed to wait on child");

    // Verify the output file was created by the initial build
    assert!(project_path.join("out/sleep/watch_init.done").exists(),
        "Watch should perform initial build");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("initial build") || stdout.contains("Processing"),
        "Watch output should mention initial build: {}", stdout);
}

#[test]
fn watch_rebuilds_on_change() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    fs::create_dir_all(project_path.join("sleep")).unwrap();
    fs::write(project_path.join("sleep/watch_change.sleep"), "0.01").unwrap();
    fs::write(
        project_path.join("rsb.toml"),
        "[processor]\nenabled = [\"sleep\"]\n"
    ).unwrap();

    let rsb_path = env!("CARGO_BIN_EXE_rsb");
    let mut child = Command::new(rsb_path)
        .current_dir(project_path)
        .args(["watch"])
        .env("NO_COLOR", "1")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("Failed to spawn rsb watch");

    // Wait for initial build
    std::thread::sleep(Duration::from_secs(2));

    // Modify the sleep file to trigger rebuild
    fs::write(project_path.join("sleep/watch_change.sleep"), "0.02").unwrap();

    // Wait for rebuild
    std::thread::sleep(Duration::from_secs(2));

    // Kill the watcher
    child.kill().expect("Failed to kill watcher");
    let output = child.wait_with_output().expect("Failed to wait on child");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Change detected"),
        "Watch should detect and report changes: {}", stdout);
}
