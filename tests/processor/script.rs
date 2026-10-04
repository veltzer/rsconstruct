use crate::common::run_rsconstruct_with_env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use tempfile::TempDir;

#[test]
fn script_valid_file() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    // script is disabled by default, so we must explicitly enable and configure it
    fs::write(
        project_path.join("rsconstruct.toml"),
        concat!(
            "[processor.script]\n",
            "command = \"true\"\n",
            "src_extensions = [\".txt\"]\n",
            "src_dirs = [\".\"]\n",
        ),
    )
    .unwrap();

    fs::write(project_path.join("test.txt"), "hello world\n").unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build", "-v"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "Build should succeed with script using 'true': stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Processing:"),
        "Should process script: {}",
        stdout
    );
}

#[test]
fn script_incremental_skip() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        concat!(
            "[processor.script]\n",
            "command = \"true\"\n",
            "src_extensions = [\".txt\"]\n",
            "src_dirs = [\".\"]\n",
        ),
    )
    .unwrap();

    fs::write(project_path.join("test.txt"), "hello world\n").unwrap();

    // First build
    let output1 = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output1.status.success());

    // Second build should skip
    let output2 =
        run_rsconstruct_with_env(project_path, &["build", "--verbose"], &[("NO_COLOR", "1")]);
    assert!(output2.status.success());
    let stdout2 = String::from_utf8_lossy(&output2.stdout);
    assert!(
        stdout2.contains("[script] Skipping (unchanged):"),
        "Second build should skip: {}",
        stdout2
    );
}

#[test]
fn script_misspelled_linter_fails_immediately() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        concat!(
            "[processor.script]\n",
            "command = \"no_such_command_xyzzy\"\n",
            "src_extensions = [\".txt\"]\n",
            "src_dirs = [\".\"]\n",
        ),
    )
    .unwrap();

    fs::write(project_path.join("test.txt"), "hello world\n").unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(
        !output.status.success(),
        "Build should fail when linter does not exist"
    );

    let exit_code = output.status.code().unwrap();
    assert_eq!(
        exit_code, 3,
        "Expected exit code 3 (TOOL_ERROR), got {}",
        exit_code
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Missing required tools") || stderr.contains("TOOL_ERROR"),
        "Should report missing tool error: {}",
        stderr
    );
}

#[test]
fn script_multi_instance_both_discover_files() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        concat!(
            "[processor.script.lint_a]\n",
            "command = \"true\"\n",
            "src_extensions = [\".txt\"]\n",
            "src_dirs = [\".\"]\n",
            "\n",
            "[processor.script.lint_b]\n",
            "command = \"true\"\n",
            "src_extensions = [\".txt\"]\n",
            "src_dirs = [\".\"]\n",
        ),
    )
    .unwrap();

    fs::write(project_path.join("test.txt"), "hello\n").unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build", "-v"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "Build should succeed: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("[script.lint_a]"),
        "Should process script.lint_a: {}",
        stdout
    );
    assert!(
        stdout.contains("[script.lint_b]"),
        "Should process script.lint_b: {}",
        stdout
    );
}

#[test]
fn script_no_project_discovered() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    // Without configuring extensions or checker, script should discover nothing
    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.script]\nsrc_dirs = [\".\"]\n",
    )
    .unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("0 products"),
        "Should discover 0 products: {}",
        stdout
    );
}

#[test]
fn script_rebuilds_when_command_file_changes() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    let script_path = project_path.join("check.sh");
    fs::write(&script_path, "#!/bin/bash\nexit 0\n").unwrap();
    fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755)).unwrap();

    fs::write(
        project_path.join("rsconstruct.toml"),
        format!(
            "[processor.script]\ncommand = \"{script}\"\nsrc_extensions = [\".txt\"]\nsrc_dirs = [\".\"]\n",
            script = script_path.display(),
        ),
    ).unwrap();
    fs::write(project_path.join("test.txt"), "hello\n").unwrap();

    // First build
    let out1 = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(out1.status.success(), "First build should succeed");

    // Second build: unchanged — should skip
    let out2 =
        run_rsconstruct_with_env(project_path, &["build", "--verbose"], &[("NO_COLOR", "1")]);
    assert!(out2.status.success());
    assert!(
        String::from_utf8_lossy(&out2.stdout).contains("Skipping"),
        "Second build should skip: {}",
        String::from_utf8_lossy(&out2.stdout),
    );

    // Modify the command script
    fs::write(&script_path, "#!/bin/bash\nexit 0\n# changed\n").unwrap();

    // Third build: command changed — must rebuild
    let out3 =
        run_rsconstruct_with_env(project_path, &["build", "--verbose"], &[("NO_COLOR", "1")]);
    assert!(out3.status.success());
    let stdout3 = String::from_utf8_lossy(&out3.stdout);
    assert!(
        stdout3.contains("Processing:"),
        "Third build must rebuild after command file change: {}",
        stdout3,
    );
}

/// `[build] command_timeout_secs` is off by default: a command that takes
/// a few seconds runs to completion and the build passes.
#[test]
fn command_timeout_is_off_by_default() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();
    fs::write(
        project_path.join("rsconstruct.toml"),
        concat!(
            "[processor.script]\n",
            "command = \"sh\"\n",
            "args = [\"-c\", \"sleep 2\"]\n",
            "src_extensions = [\".txt\"]\n",
            "src_dirs = [\".\"]\n",
        ),
    )
    .unwrap();
    fs::write(project_path.join("test.txt"), "hello\n").unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "no timeout by default, so a 2s command must pass: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// With `[build] command_timeout_secs` set, a command that overruns it is
/// killed and the build fails naming the timeout and the command.
#[test]
fn command_timeout_kills_an_overrunning_command_when_set() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();
    fs::write(
        project_path.join("rsconstruct.toml"),
        concat!(
            "[build]\n",
            "command_timeout_secs = 1\n",
            "[processor.script]\n",
            "command = \"sh\"\n",
            "args = [\"-c\", \"sleep 5\"]\n",
            "src_extensions = [\".txt\"]\n",
            "src_dirs = [\".\"]\n",
        ),
    )
    .unwrap();
    fs::write(project_path.join("test.txt"), "hello\n").unwrap();

    let start = std::time::Instant::now();
    let output = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    let elapsed = start.elapsed();
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !output.status.success(),
        "the overrunning command must fail the build: {combined}"
    );
    assert!(
        combined.contains("Command timed out after 1s and was killed"),
        "failure must name the timeout: {combined}"
    );
    assert!(
        elapsed < std::time::Duration::from_secs(4),
        "the command must be killed at the limit, not run to completion ({elapsed:?})"
    );
}
