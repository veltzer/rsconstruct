use std::fs;
use crate::common::{setup_test_project, run_rsconstruct, run_rsconstruct_with_env};

#[test]
fn status_command() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create a template
    fs::write(
        project_path.join("tera.templates/status_test.txt.tera"),
        "hello"
    ).unwrap();

    // Before building, should be STALE (use -v to see per-file status)
    let status1 = run_rsconstruct_with_env(project_path, &["-v", "status"], &[("NO_COLOR", "1")]);
    assert!(status1.status.success());
    let stdout1 = String::from_utf8_lossy(&status1.stdout);
    assert!(stdout1.contains("STALE"), "Before build, product should be STALE: {}", stdout1);

    // Build it
    let build = run_rsconstruct(project_path, &["build"]);
    assert!(build.status.success());

    // After building, should be UP-TO-DATE
    let status2 = run_rsconstruct_with_env(project_path, &["-v", "status"], &[("NO_COLOR", "1")]);
    assert!(status2.status.success());
    let stdout2 = String::from_utf8_lossy(&status2.stdout);
    assert!(stdout2.contains("UP-TO-DATE"), "After build, product should be UP-TO-DATE: {}", stdout2);

    // Check summary line
    assert!(stdout2.contains("Total"), "Status output should contain Total line");
}

#[test]
fn status_empty_project() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // No templates to process — disable all processors
    fs::write(
        project_path.join("rsconstruct.toml"),
        "\n"
    ).unwrap();

    let output = run_rsconstruct(project_path, &["status"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("No products discovered"), "Empty project should show 'No products discovered': {}", stdout);
}
