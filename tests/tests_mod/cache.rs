use std::fs;
use crate::common::{setup_test_project, run_rsb, run_rsb_with_env};

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
        project_path.join("templates.tera/cached.txt.tera"),
        "{% set c = load_python(path='config/cache_test.py') %}{{ c.value }}"
    ).unwrap();

    // Build to populate cache
    let output = run_rsb(project_path, &["build"]);
    assert!(output.status.success());
    assert!(project_path.join("cached.txt").exists());
    assert!(project_path.join(".rsbuild/db.redb").exists());
    assert!(project_path.join(".rsbuild/objects").exists());

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
    // .rsbuild/ exists (fresh db) but objects dir is gone
    assert!(!project_path.join(".rsbuild").join("objects").exists());

    // Cache size after clear should be 0
    let size_after = run_rsb(project_path, &["cache", "size"]);
    assert!(size_after.status.success());
    let size_after_stdout = String::from_utf8_lossy(&size_after.stdout);
    assert!(size_after_stdout.contains("0 B"));
    assert!(size_after_stdout.contains("0 objects"));
}

#[test]
fn cache_list_shows_entries() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    fs::create_dir_all(project_path.join("sleep")).unwrap();
    fs::write(project_path.join("sleep/list_test.sleep"), "0.01").unwrap();
    fs::write(
        project_path.join("rsbuild.toml"),
        "[processor]\nenabled = [\"sleep\"]\n"
    ).unwrap();

    // Build to populate cache
    let build = run_rsb(project_path, &["build"]);
    assert!(build.status.success());

    // List cache — output is JSON
    let output = run_rsb_with_env(project_path, &["cache", "list"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let entries: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("Cache list should be valid JSON: {}\nOutput: {}", e, stdout));
    let arr = entries.as_array().expect("Cache list should be a JSON array");
    assert!(!arr.is_empty(), "Cache list should have entries after build");
    let first = &arr[0];
    assert!(first["cache_key"].as_str().unwrap().contains("sleep"),
        "Cache entry should contain processor name: {}", first);
    // Checkers have empty outputs - the cache entry itself is the success record
    let outputs = first["outputs"].as_array().expect("outputs should be an array");
    assert!(outputs.is_empty(), "Checker cache entry should have empty outputs: {}", first);
}

#[test]
fn cache_list_empty() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsbuild.toml"),
        "[processor]\nenabled = []\n"
    ).unwrap();

    // Empty cache should produce an empty JSON array
    let output = run_rsb_with_env(project_path, &["cache", "list"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let entries: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("Cache list should be valid JSON: {}\nOutput: {}", e, stdout));
    let arr = entries.as_array().expect("Cache list should be a JSON array");
    assert!(arr.is_empty(), "Empty cache should produce an empty JSON array: {}", stdout);
}

#[test]
fn cache_stats_empty() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsbuild.toml"),
        "[processor]\nenabled = []\n"
    ).unwrap();

    let output = run_rsb_with_env(project_path, &["cache", "stats"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Cache is empty"), "Expected 'Cache is empty' message, got: {}", stdout);
}

#[test]
fn cache_stats_after_build() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    fs::create_dir_all(project_path.join("sleep")).unwrap();
    fs::write(project_path.join("sleep/stats_test.sleep"), "0.01").unwrap();
    fs::write(
        project_path.join("rsbuild.toml"),
        "[processor]\nenabled = [\"sleep\"]\n"
    ).unwrap();

    // Build to populate cache
    let build = run_rsb(project_path, &["build"]);
    assert!(build.status.success());

    // Check stats
    let output = run_rsb_with_env(project_path, &["cache", "stats"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("sleep"), "Expected processor name in stats, got: {}", stdout);
    assert!(stdout.contains("entries"), "Expected 'entries' in stats, got: {}", stdout);
}

#[test]
fn cache_stats_json() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    fs::create_dir_all(project_path.join("sleep")).unwrap();
    fs::write(project_path.join("sleep/json_test.sleep"), "0.01").unwrap();
    fs::write(
        project_path.join("rsbuild.toml"),
        "[processor]\nenabled = [\"sleep\"]\n"
    ).unwrap();

    // Build to populate cache
    let build = run_rsb(project_path, &["build"]);
    assert!(build.status.success());

    // Verify cache stats outputs valid JSON
    let output = run_rsb_with_env(project_path, &["--json", "cache", "stats"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("Cache stats JSON should be valid: {}\nOutput: {}", e, stdout));
    assert!(parsed.is_object(), "Expected JSON object, got: {}", stdout);
    assert!(parsed.get("sleep").is_some(), "Expected 'sleep' key in stats JSON, got: {}", stdout);
}
