use crate::common::{run_rsconstruct, run_rsconstruct_with_env, setup_test_project};
use std::fs;
use tempfile::TempDir;

#[test]
fn missing_config_returns_config_error() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    // No rsconstruct.toml in directory
    let output = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(!output.status.success());

    let exit_code = output.status.code().unwrap();
    assert_eq!(
        exit_code, 2,
        "Expected exit code 2 (CONFIG_ERROR), got {}",
        exit_code
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("CONFIG_ERROR"),
        "Expected CONFIG_ERROR in stderr, got: {}",
        stderr
    );
}

#[test]
fn init_already_exists_returns_config_error() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    // Create rsconstruct.toml first
    fs::write(project_path.join("rsconstruct.toml"), "# existing").unwrap();

    let output = run_rsconstruct_with_env(project_path, &["init"], &[("NO_COLOR", "1")]);
    assert!(!output.status.success());

    let exit_code = output.status.code().unwrap();
    assert_eq!(
        exit_code, 2,
        "Expected exit code 2 (CONFIG_ERROR), got {}",
        exit_code
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("CONFIG_ERROR"),
        "Expected CONFIG_ERROR in stderr, got: {}",
        stderr
    );
}

#[test]
fn success_returns_zero() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    let output = run_rsconstruct(project_path, &["init"]);
    assert!(output.status.success());

    let exit_code = output.status.code().unwrap();
    assert_eq!(
        exit_code, 0,
        "Expected exit code 0 (SUCCESS), got {}",
        exit_code
    );
}

#[test]
fn unknown_processor_returns_config_error() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    let output = run_rsconstruct_with_env(
        project_path,
        &["build", "-p", "nonexistent"],
        &[("NO_COLOR", "1")],
    );
    assert!(!output.status.success());

    let exit_code = output.status.code().unwrap();
    assert_eq!(
        exit_code, 2,
        "Expected exit code 2 (CONFIG_ERROR), got {}",
        exit_code
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("CONFIG_ERROR"),
        "Expected CONFIG_ERROR in stderr, got: {}",
        stderr
    );
}

/// A bare `clean` lists its subcommands, like every other command that takes
/// one, instead of failing as a build error with a hand-written usage line.
#[test]
fn bare_clean_lists_its_subcommands() {
    let temp_dir = setup_test_project();
    let output = run_rsconstruct_with_env(temp_dir.path(), &["clean"], &[("NO_COLOR", "1")]);
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    // A subcommand is a line's first word: a bare `contains` would also
    // match the name inside another subcommand's description.
    let listed: Vec<&str> = text
        .lines()
        .filter(|line| line.starts_with("  "))
        .filter_map(|line| line.split_whitespace().next())
        .collect();
    assert_eq!(
        listed,
        ["all", "git", "outputs", "unknown"],
        "bare clean must list exactly its subcommands:\n{text}"
    );
    assert!(
        !text.contains("EXIT_BUILD_ERROR"),
        "a missing subcommand is not a build error:\n{text}"
    );
    assert!(
        !text.contains("Options:"),
        "the subcommand list leaves the flags to --help:\n{text}"
    );
    assert_eq!(output.status.code(), Some(2), "{text}");
}

/// Every command that needs `rsconstruct.toml` says so the same way when
/// there is none: a config error, exit 2. Some used to carry on with an
/// empty config (`tag check` exited 0), some failed with an IO error about
/// the file (exit 5), some with a plain error (exit 1).
#[test]
fn commands_that_need_a_config_agree_on_a_missing_one() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    for args in [
        &["status"][..],
        &["tag", "check"],
        &["term", "stats"],
        &["symlink-install"],
        &["smart", "reset"],
        &["processor", "enable", "processor.checker.ruff"],
        &["processor", "config", "processor.checker.ruff"],
        &["tool", "check"],
        &["tool", "lock"],
        &["toml", "check"],
    ] {
        let output = run_rsconstruct_with_env(project_path, args, &[("NO_COLOR", "1")]);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(2), "{args:?}: {stderr}");
        assert!(stderr.contains("rsconstruct.toml"), "{args:?}: {stderr}");
    }
}

/// A value of the wrong kind in a processor or analyzer stanza is a config
/// error naming the stanza, from `toml check` as from a build. `toml check`
/// used to call such a file valid, and the build then failed with exit 1
/// and "TOML parse error at line 1, column 1".
#[test]
fn a_wrong_value_in_a_stanza_is_a_config_error_everywhere() {
    for (config, section) in [
        (
            "[processor.checker.ascii]\nmax_jobs = -1\n",
            "[processor.checker.ascii]",
        ),
        ("[analyzer.cpp]\ninclude_paths = 3\n", "[analyzer.cpp]"),
    ] {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let project_path = temp_dir.path();
        fs::write(project_path.join("rsconstruct.toml"), config).unwrap();
        for args in [&["toml", "check"][..], &["build"], &["status"]] {
            let output = run_rsconstruct_with_env(project_path, args, &[("NO_COLOR", "1")]);
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert_eq!(output.status.code(), Some(2), "{args:?}: {stderr}");
            assert!(stderr.contains(section), "{args:?}: {stderr}");
            assert!(!stderr.contains("line 1"), "{args:?}: {stderr}");
        }
    }
}
