use crate::common::{setup_test_project, run_rsbuild_with_env};

#[test]
fn complete_bash_generates_output() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    let output = run_rsbuild_with_env(project_path, &["complete", "bash"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(), "complete bash failed: {}", String::from_utf8_lossy(&output.stderr));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.is_empty(), "Expected completion output for bash");
    assert!(stdout.contains("rsbuild"), "Expected 'rsbuild' in bash completion script");
}

#[test]
fn complete_zsh_generates_output() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    let output = run_rsbuild_with_env(project_path, &["complete", "zsh"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(), "complete zsh failed: {}", String::from_utf8_lossy(&output.stderr));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.is_empty(), "Expected completion output for zsh");
}

#[test]
fn complete_fish_generates_output() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    let output = run_rsbuild_with_env(project_path, &["complete", "fish"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(), "complete fish failed: {}", String::from_utf8_lossy(&output.stderr));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.is_empty(), "Expected completion output for fish");
}

#[test]
fn complete_from_config() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // setup_test_project doesn't set completions config, add it
    let config = "[processor]\nenabled = [\"tera\"]\n\n[completions]\nshells = [\"bash\"]\n";
    std::fs::write(project_path.join("rsbuild.toml"), config).expect("Failed to write rsbuild.toml");

    // Running complete without arguments should use config
    let output = run_rsbuild_with_env(project_path, &["complete"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(), "complete from config failed: {}", String::from_utf8_lossy(&output.stderr));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.is_empty(), "Expected completion output from config");
    assert!(stdout.contains("rsbuild"), "Expected 'rsbuild' in completion output");
}
