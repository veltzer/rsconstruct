use std::fs;
use crate::common::{setup_test_project, run_rsb_with_env};

#[test]
fn processor_list_shows_enabled() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    let output = run_rsb_with_env(project_path, &["processor", "list"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(), "processor list failed: {}", String::from_utf8_lossy(&output.stderr));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("template"), "Expected template processor in list");
    assert!(stdout.contains("enabled"), "Expected 'enabled' status for template");
}

#[test]
fn processor_list_shows_disabled() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    let output = run_rsb_with_env(project_path, &["processor", "list"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Only template is enabled in setup_test_project, others should be disabled
    assert!(stdout.contains("disabled"), "Expected some disabled processors in list");
}

#[test]
fn processor_auto_detects_template() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Write a template file so the template processor is detected
    fs::write(
        project_path.join("templates/test.txt.tera"),
        "hello"
    ).expect("Failed to write template");

    let output = run_rsb_with_env(project_path, &["processor", "auto"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(), "processor auto failed: {}", String::from_utf8_lossy(&output.stderr));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("template"), "Expected template in auto-detect output");
    assert!(stdout.contains("detected"), "Expected 'detected' for template processor");
}

#[test]
fn processor_files_shows_products() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Write a template so there's at least one product
    fs::write(
        project_path.join("config/test.py"),
        "value = 42"
    ).expect("Failed to write config");
    fs::write(
        project_path.join("templates/output.txt.tera"),
        "{% set c = load_python(path='config/test.py') %}{{ c.value }}"
    ).expect("Failed to write template");

    let output = run_rsb_with_env(project_path, &["processor", "files"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(), "processor files failed: {}", String::from_utf8_lossy(&output.stderr));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("[template]"), "Expected [template] header in output");
    assert!(stdout.contains("output.txt"), "Expected output file in listing");
}

#[test]
fn processor_files_no_files_message() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // No template files written, so no products
    let output = run_rsb_with_env(project_path, &["processor", "files"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("No files discovered") || stdout.contains("(no files)"),
        "Expected empty message, got: {}", stdout);
}

#[test]
fn processor_files_unknown_processor_fails() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    let output = run_rsb_with_env(project_path, &["processor", "files", "nonexistent"], &[("NO_COLOR", "1")]);
    assert!(!output.status.success(), "Expected failure for unknown processor");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Unknown processor"), "Expected 'Unknown processor' error, got: {}", stderr);
}

#[test]
fn processor_all_shows_descriptions() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    let output = run_rsb_with_env(project_path, &["processor", "all"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(), "processor all failed: {}", String::from_utf8_lossy(&output.stderr));

    let stdout = String::from_utf8_lossy(&output.stdout);
    // processor all shows descriptions with " — " separator
    assert!(stdout.contains("template"), "Expected template processor");
}
