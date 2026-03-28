use std::fs;
use crate::common::{setup_test_project, run_rsconstruct_with_env};

#[test]
fn config_show_outputs_toml() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    let output = run_rsconstruct_with_env(project_path, &["config", "show"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(), "config show failed: {}", String::from_utf8_lossy(&output.stderr));

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Should contain TOML section headers from the merged config
    assert!(stdout.contains("[build]"), "Expected [build] section");
    assert!(stdout.contains("[cache]"), "Expected [cache] section");
    // Processor config is under [processor.NAME] sections
    assert!(stdout.contains("[processor.tera]") || stdout.contains("processor"), "Expected processor config");
}

#[test]
fn config_show_reflects_project_config() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    let output = run_rsconstruct_with_env(project_path, &["config", "show"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    // setup_test_project enables only "tera", so the output should reflect that
    assert!(stdout.contains("tera"), "Expected tera processor in config output");
}

#[test]
fn config_show_default_outputs_toml() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    let output = run_rsconstruct_with_env(project_path, &["config", "show-default"], &[("NO_COLOR", "1")]);
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

    let output = run_rsconstruct_with_env(project_path, &["config", "show"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    // The annotate_config function adds comments for constrained values
    assert!(stdout.contains("# 0 = auto-detect CPU cores"), "Expected parallel annotation");
    assert!(stdout.contains("# options: hardlink, copy"), "Expected restore_method annotation");
}

#[test]
fn config_vars_substitution_array() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Write config with vars section and array variable
    fs::write(
        project_path.join("rsconstruct.toml"),
        r#"
[vars]
my_excludes = ["/kernel/", "/vendor/"]

[processor.tera]

[processor.cppcheck]
exclude_dirs = "${my_excludes}"
"#
    ).unwrap();

    let output = run_rsconstruct_with_env(project_path, &["config", "show"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(), "config show failed: {}", String::from_utf8_lossy(&output.stderr));

    let stdout = String::from_utf8_lossy(&output.stdout);
    // The variable should be substituted with the array value
    assert!(stdout.contains("/kernel/"), "Expected /kernel/ in resolved config");
    assert!(stdout.contains("/vendor/"), "Expected /vendor/ in resolved config");
}

#[test]
fn config_vars_substitution_string() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Write config with a string variable
    fs::write(
        project_path.join("rsconstruct.toml"),
        r#"
[vars]
my_dir = "custom_templates"

[processor.tera]
scan_dir = "${my_dir}"
"#
    ).unwrap();

    let output = run_rsconstruct_with_env(project_path, &["config", "show"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(), "config show failed: {}", String::from_utf8_lossy(&output.stderr));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("custom_templates"), "Expected custom_templates in resolved config");
}

#[test]
fn config_vars_multiple_uses() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Use the same variable in multiple places
    fs::write(
        project_path.join("rsconstruct.toml"),
        r#"
[vars]
shared_excludes = ["/out/", "/build/"]

[processor.tera]

[processor.cppcheck]
exclude_dirs = "${shared_excludes}"

[processor.shellcheck]
exclude_dirs = "${shared_excludes}"
"#
    ).unwrap();

    let output = run_rsconstruct_with_env(project_path, &["config", "show"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(), "config show failed: {}", String::from_utf8_lossy(&output.stderr));

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Both processors should have the resolved value
    assert!(stdout.contains("/out/"), "Expected /out/ in resolved config");
    assert!(stdout.contains("/build/"), "Expected /build/ in resolved config");
}

#[test]
fn config_vars_undefined_variable_error() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Reference an undefined variable
    fs::write(
        project_path.join("rsconstruct.toml"),
        r#"
[processor.tera]

[processor.cppcheck]
exclude_dirs = "${undefined_var}"
"#
    ).unwrap();

    let output = run_rsconstruct_with_env(project_path, &["config", "show"], &[("NO_COLOR", "1")]);
    assert!(!output.status.success(), "Expected config show to fail for undefined variable");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("undefined_var"), "Expected error message to mention undefined variable");
}

#[test]
fn config_vars_no_vars_section() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Config without vars section should work normally
    fs::write(
        project_path.join("rsconstruct.toml"),
        r#"
[processor.tera]
scan_dir = "templates"
"#
    ).unwrap();

    let output = run_rsconstruct_with_env(project_path, &["config", "show"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(), "config show failed: {}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn config_validate_ok() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create a template file so tera has matching files
    fs::write(
        project_path.join("tera.templates/test.txt.tera"),
        "hello"
    ).unwrap();

    let output = run_rsconstruct_with_env(project_path, &["config", "validate"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(), "config validate failed: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Config OK"), "Expected 'Config OK', got: {}", stdout);
}

#[test]
fn config_validate_unknown_processor() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.nonexistent_proc]\n"
    ).unwrap();

    let output = run_rsconstruct_with_env(project_path, &["config", "validate"], &[("NO_COLOR", "1")]);
    assert!(!output.status.success(), "Expected config validate to fail for unknown processor");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("nonexistent_proc"), "Expected error about unknown processor, got: {}", stdout);
    assert!(stdout.contains("ERROR"), "Expected ERROR label, got: {}", stdout);
}

#[test]
fn config_validate_no_matching_files_warning() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Enable yamllint processor but don't create any .yml files
    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.yamllint]\n"
    ).unwrap();

    let output = run_rsconstruct_with_env(project_path, &["config", "validate"], &[("NO_COLOR", "1")]);
    // Should succeed (warnings only, no errors)
    assert!(output.status.success(), "config validate should succeed with only warnings: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("WARNING"), "Expected WARNING label, got: {}", stdout);
    assert!(stdout.contains("no matching files"), "Expected 'no matching files' warning, got: {}", stdout);
}

#[test]
fn config_validate_json() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Enable yamllint processor but don't create any .yml files (to get a warning)
    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.yamllint]\n"
    ).unwrap();

    let output = run_rsconstruct_with_env(project_path, &["--json", "config", "validate"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("Config validate JSON should be valid: {}\nOutput: {}", e, stdout));
    assert!(parsed.is_array(), "Expected JSON array, got: {}", stdout);
    let arr = parsed.as_array().unwrap();
    assert!(!arr.is_empty(), "Expected at least one issue, got: {}", stdout);
    let first = &arr[0];
    assert!(first.get("severity").is_some(), "Expected severity field");
    assert!(first.get("message").is_some(), "Expected message field");
}
