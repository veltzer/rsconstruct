mod common;

use std::fs;
use common::{setup_test_project, run_rsb, run_rsb_with_env};

#[test]
fn cache_operations() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create a simple template
    fs::write(
        project_path.join("config/cache_test.py"),
        "value = 'cached'"
    ).unwrap();

    fs::write(
        project_path.join("templates/cached.txt.tera"),
        "{% set c = load_python(path='config/cache_test.py') %}{{ c.value }}"
    ).unwrap();

    // Build to populate cache
    let output = run_rsb(project_path, &["build"]);
    assert!(output.status.success());
    assert!(project_path.join("cached.txt").exists());
    assert!(project_path.join(".rsb/index.json").exists());
    assert!(project_path.join(".rsb/objects").exists());

    // Check cache size reports objects
    let size_output = run_rsb(project_path, &["cache", "size"]);
    assert!(size_output.status.success());
    let size_stdout = String::from_utf8_lossy(&size_output.stdout);
    assert!(size_stdout.contains("Cache size:"));
    assert!(size_stdout.contains("objects"));
    // Should have at least 1 object
    assert!(!size_stdout.contains("0 objects"), "Cache should have objects after build");

    // Delete the output file, then rebuild — should restore from cache
    fs::remove_file(project_path.join("cached.txt")).unwrap();
    assert!(!project_path.join("cached.txt").exists());

    let restore_output = run_rsb_with_env(project_path, &["build", "--verbose"], &[("NO_COLOR", "1")]);
    assert!(restore_output.status.success());
    let restore_stdout = String::from_utf8_lossy(&restore_output.stdout);
    assert!(restore_stdout.contains("Restored from cache:"));
    assert!(project_path.join("cached.txt").exists());

    // Verify restored content is correct
    let content = fs::read_to_string(project_path.join("cached.txt")).unwrap();
    assert_eq!(content.trim(), "cached");

    // Trim cache (nothing unreferenced, so 0 removed)
    let trim_output = run_rsb(project_path, &["cache", "trim"]);
    assert!(trim_output.status.success());
    let trim_stdout = String::from_utf8_lossy(&trim_output.stdout);
    assert!(trim_stdout.contains("0 unreferenced objects"));

    // Clear cache entirely
    let clear_output = run_rsb(project_path, &["cache", "clear"]);
    assert!(clear_output.status.success());
    assert!(!project_path.join(".rsb").exists());

    // Cache size after clear should be 0
    let size_after = run_rsb(project_path, &["cache", "size"]);
    assert!(size_after.status.success());
    let size_after_stdout = String::from_utf8_lossy(&size_after.stdout);
    assert!(size_after_stdout.contains("0 bytes"));
    assert!(size_after_stdout.contains("0 objects"));
}

#[test]
fn cache_list_shows_entries() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    fs::create_dir_all(project_path.join("sleep")).unwrap();
    fs::write(project_path.join("sleep/list_test.sleep"), "0.01").unwrap();
    fs::write(
        project_path.join("rsb.toml"),
        "[processor]\nenabled = [\"sleep\"]\n"
    ).unwrap();

    // Build to populate cache
    let build = run_rsb(project_path, &["build"]);
    assert!(build.status.success());

    // List cache
    let output = run_rsb_with_env(project_path, &["cache", "list"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("sleep"), "Cache list should contain processor name: {}", stdout);
    assert!(stdout.contains("ok"), "Cache list should show 'ok' for existing objects: {}", stdout);
    assert!(stdout.contains("cache entries"), "Cache list should show entry count: {}", stdout);
}

#[test]
fn cache_list_empty() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsb.toml"),
        "[processor]\nenabled = []\n"
    ).unwrap();

    let output = run_rsb_with_env(project_path, &["cache", "list"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("No cache entries"), "Empty cache should show 'No cache entries': {}", stdout);
}
