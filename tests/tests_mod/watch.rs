use std::fs;
use std::process::Command;
use std::time::Duration;
use crate::common::setup_test_project;

#[test]
fn watch_does_initial_build() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("tera.templates/watch_init.txt.tera"),
        "hello"
    ).unwrap();

    let rsconstruct_path = env!("CARGO_BIN_EXE_rsconstruct");
    let mut child = Command::new(rsconstruct_path)
        .current_dir(project_path)
        .args(["watch"])
        .env("NO_COLOR", "1")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("Failed to spawn rsconstruct watch");

    // Wait for initial build to complete
    std::thread::sleep(Duration::from_secs(2));

    // Kill the watcher
    child.kill().expect("Failed to kill watcher");
    let output = child.wait_with_output().expect("Failed to wait on child");

    // Verify via stdout that the build processed the file
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("initial build") || stdout.contains("Processing"),
        "Watch output should mention initial build: {}", stdout);
}

#[test]
fn watch_rebuilds_on_change() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("tera.templates/watch_change.txt.tera"),
        "hello"
    ).unwrap();

    let rsconstruct_path = env!("CARGO_BIN_EXE_rsconstruct");
    let mut child = Command::new(rsconstruct_path)
        .current_dir(project_path)
        .args(["watch"])
        .env("NO_COLOR", "1")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("Failed to spawn rsconstruct watch");

    // Wait for initial build
    std::thread::sleep(Duration::from_secs(2));

    // Modify the template file to trigger rebuild
    fs::write(project_path.join("tera.templates/watch_change.txt.tera"), "changed").unwrap();

    // Wait for rebuild
    std::thread::sleep(Duration::from_secs(2));

    // Kill the watcher
    child.kill().expect("Failed to kill watcher");
    let output = child.wait_with_output().expect("Failed to wait on child");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Change detected"),
        "Watch should detect and report changes: {}", stdout);
}
