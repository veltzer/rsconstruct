use crate::common::{rsconstruct_command, setup_test_project};
use std::fs;
use std::time::Duration;

#[test]
fn watch_does_initial_build() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("tera.templates/watch_init.txt.tera"),
        "hello",
    )
    .unwrap();

    let mut child = rsconstruct_command(project_path)
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
    assert!(
        stdout.contains("initial build") || stdout.contains("Processing"),
        "Watch output should mention initial build: {stdout}"
    );
}

#[test]
fn watch_rebuilds_on_change() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("tera.templates/watch_change.txt.tera"),
        "hello",
    )
    .unwrap();

    let mut child = rsconstruct_command(project_path)
        .args(["watch"])
        .env("NO_COLOR", "1")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("Failed to spawn rsconstruct watch");

    // Wait for initial build
    std::thread::sleep(Duration::from_secs(2));

    // Modify the template file to trigger rebuild
    fs::write(
        project_path.join("tera.templates/watch_change.txt.tera"),
        "changed",
    )
    .unwrap();

    // Wait for rebuild
    std::thread::sleep(Duration::from_secs(2));

    // Kill the watcher
    child.kill().expect("Failed to kill watcher");
    let output = child.wait_with_output().expect("Failed to wait on child");

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Exactly one: the build's own reads of rsconstruct.toml and the
    // sources are reported as access events, and counting those as changes
    // made watch mode rebuild in a loop.
    assert_eq!(
        stdout.matches("Change detected").count(),
        1,
        "one edit must trigger one rebuild: {stdout}"
    );
}

/// Editors save by writing a new file and renaming it over the old one. A
/// watch on a single file follows the inode, so after the first such save
/// of rsconstruct.toml the watch was on a file that no longer exists and
/// every later save went unnoticed.
#[test]
fn watch_keeps_seeing_config_saves_made_by_rename() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();
    fs::write(project_path.join("tera.templates/w.txt.tera"), "hello").unwrap();
    let config = fs::read_to_string(project_path.join("rsconstruct.toml")).unwrap();

    let mut child = rsconstruct_command(project_path)
        .args(["watch"])
        .env("NO_COLOR", "1")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("Failed to spawn rsconstruct watch");
    std::thread::sleep(Duration::from_secs(2));

    for round in 1..=2 {
        let tmp = project_path.join("rsconstruct.toml.new");
        fs::write(&tmp, format!("{config}# save {round}\n")).unwrap();
        fs::rename(&tmp, project_path.join("rsconstruct.toml")).unwrap();
        std::thread::sleep(Duration::from_secs(2));
    }

    child.kill().expect("Failed to kill watcher");
    let output = child.wait_with_output().expect("Failed to wait on child");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout.matches("Change detected").count(),
        2,
        "each save by rename must trigger a rebuild: {stdout}"
    );
}
