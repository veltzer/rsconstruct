mod common;

use std::fs;
use common::{setup_test_project, run_rsb, run_rsb_with_env};

#[test]
fn status_command() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create a sleep file
    fs::create_dir_all(project_path.join("sleep")).unwrap();
    fs::write(project_path.join("sleep/status_test.sleep"), "0.01").unwrap();
    fs::write(
        project_path.join("rsb.toml"),
        "[processor]\nenabled = [\"sleep\"]\n"
    ).unwrap();

    // Before building, should be STALE
    let status1 = run_rsb_with_env(project_path, &["status"], &[("NO_COLOR", "1")]);
    assert!(status1.status.success());
    let stdout1 = String::from_utf8_lossy(&status1.stdout);
    assert!(stdout1.contains("STALE"), "Before build, product should be STALE: {}", stdout1);

    // Build it
    let build = run_rsb(project_path, &["build"]);
    assert!(build.status.success());

    // After building, should be UP-TO-DATE
    let status2 = run_rsb_with_env(project_path, &["status"], &[("NO_COLOR", "1")]);
    assert!(status2.status.success());
    let stdout2 = String::from_utf8_lossy(&status2.stdout);
    assert!(stdout2.contains("UP-TO-DATE"), "After build, product should be UP-TO-DATE: {}", stdout2);

    // Delete output, should be RESTORABLE (cache still exists)
    fs::remove_file(project_path.join("out/sleep/status_test.done")).unwrap();
    let status3 = run_rsb_with_env(project_path, &["status"], &[("NO_COLOR", "1")]);
    assert!(status3.status.success());
    let stdout3 = String::from_utf8_lossy(&status3.stdout);
    assert!(stdout3.contains("RESTORABLE"), "After deleting output, product should be RESTORABLE: {}", stdout3);

    // Check summary line
    assert!(stdout3.contains("Summary"), "Status output should contain Summary line");
}

#[test]
fn status_empty_project() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // No sleep dir, no templates to process — disable all processors
    fs::write(
        project_path.join("rsb.toml"),
        "[processor]\nenabled = []\n"
    ).unwrap();

    let output = run_rsb(project_path, &["status"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("No products discovered"), "Empty project should show 'No products discovered': {}", stdout);
}
