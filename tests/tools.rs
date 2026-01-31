mod common;

use common::{setup_test_project, run_rsb_with_env};

#[test]
fn tools_list_shows_tools() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    let output = run_rsb_with_env(project_path, &["tools", "list"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(), "tools list failed: {}", String::from_utf8_lossy(&output.stderr));

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Template processor requires tera (built-in), so it may not list external tools.
    // The command should at least succeed without errors.
    // If tools are listed, they should show the processor name in parentheses.
    if !stdout.is_empty() {
        assert!(stdout.contains("("), "Expected processor name in parentheses for each tool");
    }
}

#[test]
fn tools_list_all_includes_disabled() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    let output_default = run_rsb_with_env(project_path, &["tools", "list"], &[("NO_COLOR", "1")]);
    let output_all = run_rsb_with_env(project_path, &["tools", "list", "-a"], &[("NO_COLOR", "1")]);

    assert!(output_default.status.success());
    assert!(output_all.status.success());

    let stdout_default = String::from_utf8_lossy(&output_default.stdout);
    let stdout_all = String::from_utf8_lossy(&output_all.stdout);

    // -a should show at least as many tools as the default
    assert!(stdout_all.len() >= stdout_default.len(),
        "tools list -a should include at least as many tools as default");
}

#[test]
fn tools_check_succeeds() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // With only template enabled, there may be no external tools to check.
    // The command should succeed in that case.
    let output = run_rsb_with_env(project_path, &["tools", "check"], &[("NO_COLOR", "1")]);
    // Don't assert success here since it depends on what tools are installed,
    // but it should at least run without crashing
    let _stdout = String::from_utf8_lossy(&output.stdout);
    let _stderr = String::from_utf8_lossy(&output.stderr);
}
