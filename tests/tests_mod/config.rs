use std::fs;
use crate::common::{setup_test_project, run_rsb_with_env};

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

#[test]
fn config_vars_substitution_array() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Write config with vars section and array variable
    fs::write(
        project_path.join("rsb.toml"),
        r#"
[vars]
my_excludes = ["/kernel/", "/vendor/"]

[processor]
enabled = ["tera"]

[processor.cpplint]
exclude_dirs = "${my_excludes}"
"#
    ).unwrap();

    let output = run_rsb_with_env(project_path, &["config", "show"], &[("NO_COLOR", "1")]);
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
        project_path.join("rsb.toml"),
        r#"
[vars]
my_dir = "custom_templates"

[processor]
enabled = ["tera"]

[processor.tera]
scan_dir = "${my_dir}"
"#
    ).unwrap();

    let output = run_rsb_with_env(project_path, &["config", "show"], &[("NO_COLOR", "1")]);
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
        project_path.join("rsb.toml"),
        r#"
[vars]
shared_excludes = ["/out/", "/build/"]

[processor]
enabled = ["tera"]

[processor.cpplint]
exclude_dirs = "${shared_excludes}"

[processor.shellcheck]
exclude_dirs = "${shared_excludes}"
"#
    ).unwrap();

    let output = run_rsb_with_env(project_path, &["config", "show"], &[("NO_COLOR", "1")]);
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
        project_path.join("rsb.toml"),
        r#"
[processor]
enabled = ["tera"]

[processor.cpplint]
exclude_dirs = "${undefined_var}"
"#
    ).unwrap();

    let output = run_rsb_with_env(project_path, &["config", "show"], &[("NO_COLOR", "1")]);
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
        project_path.join("rsb.toml"),
        r#"
[processor]
enabled = ["tera"]

[processor.tera]
scan_dir = "templates"
"#
    ).unwrap();

    let output = run_rsb_with_env(project_path, &["config", "show"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(), "config show failed: {}", String::from_utf8_lossy(&output.stderr));
}
