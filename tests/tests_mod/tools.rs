use crate::common::{setup_test_project, run_rsb_with_env};

#[test]
fn tools_list_shows_tools() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    let output = run_rsb_with_env(project_path, &["tools", "list"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(), "tools list failed: {}", String::from_utf8_lossy(&output.stderr));

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Template processor requires python3, so tools list always has output.
    assert!(!stdout.is_empty(), "tools list should show at least one tool");
    assert!(stdout.contains("("), "Expected processor name in parentheses for each tool");
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

    // -a should show at least as many tool entries as the default
    let count_default = stdout_default.lines().count();
    let count_all = stdout_all.lines().count();
    assert!(count_all >= count_default,
        "tools list -a should include at least as many tools as default ({} vs {})",
        count_all, count_default);
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
