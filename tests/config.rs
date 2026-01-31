mod common;

use common::{setup_test_project, run_rsb_with_env};

#[test]
fn config_show_outputs_toml() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    let output = run_rsb_with_env(project_path, &["config", "show"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(), "config show failed: {}", String::from_utf8_lossy(&output.stderr));

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Should contain TOML section headers from the merged config
    assert!(stdout.contains("[build]"), "Expected [build] section");
    assert!(stdout.contains("[processor]"), "Expected [processor] section");
    assert!(stdout.contains("[cache]"), "Expected [cache] section");
}

#[test]
fn config_show_reflects_project_config() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    let output = run_rsb_with_env(project_path, &["config", "show"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    // setup_test_project enables only "template", so the output should reflect that
    assert!(stdout.contains("template"), "Expected template processor in config output");
}

#[test]
fn config_show_default_outputs_toml() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    let output = run_rsb_with_env(project_path, &["config", "show-default"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(), "config show-default failed: {}", String::from_utf8_lossy(&output.stderr));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("[build]"), "Expected [build] section in defaults");
    assert!(stdout.contains("[processor]"), "Expected [processor] section in defaults");
    assert!(stdout.contains("[cache]"), "Expected [cache] section in defaults");
}

#[test]
fn config_show_includes_annotations() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    let output = run_rsb_with_env(project_path, &["config", "show"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    // The annotate_config function adds comments for constrained values
    assert!(stdout.contains("# 0 = auto-detect CPU cores"), "Expected parallel annotation");
    assert!(stdout.contains("# options: hardlink, copy"), "Expected restore_method annotation");
}
