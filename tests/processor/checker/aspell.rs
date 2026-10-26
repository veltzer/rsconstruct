use crate::common::{require_tool, run_rsconstruct_with_env};
use std::fs;
use tempfile::TempDir;

#[test]
fn aspell_valid_file() {
    require_tool("aspell");

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.checker.aspell]\nsrc_dirs = [\".\"]\n",
    )
    .unwrap();

    // Create aspell config
    fs::write(project_path.join(".aspell.conf"), "lang en_US\n").unwrap();

    fs::write(
        project_path.join("doc.md"),
        "# Hello World\n\nThis is a simple test document with correct spelling.\n",
    )
    .unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build", "-v"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "Build should succeed with correctly spelled file: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Processing:"),
        "Should process aspell: {}",
        stdout
    );
}

#[test]
fn aspell_incremental_skip() {
    require_tool("aspell");

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.checker.aspell]\nsrc_dirs = [\".\"]\n",
    )
    .unwrap();

    fs::write(project_path.join(".aspell.conf"), "lang en_US\n").unwrap();

    fs::write(project_path.join("doc.md"), "# Hello\n\nThis is correct.\n").unwrap();

    // First build
    let output1 = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output1.status.success());

    // Second build should skip
    let output2 =
        run_rsconstruct_with_env(project_path, &["build", "--verbose"], &[("NO_COLOR", "1")]);
    assert!(output2.status.success());
    let stdout2 = String::from_utf8_lossy(&output2.stdout);
    assert!(
        stdout2.contains("[processor.checker.aspell] Skipping (unchanged):"),
        "Second build should skip: {}",
        stdout2
    );
}

/// A src_dirs entry that does not exist is a config error naming the
/// processor and the entry, not a silent skip.
#[test]
fn aspell_nonexistent_src_dir_is_config_error() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.checker.aspell]\nsrc_dirs = [\"aspell_docs\"]\n",
    )
    .unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(2),
        "a missing src_dirs entry must fail as a config error: {stderr}"
    );
    assert!(
        stderr.contains("processor.checker.aspell") && stderr.contains("aspell_docs"),
        "error must name the processor and the entry: {stderr}"
    );
}

/// A `conf` other than the default is an input of every product: editing
/// it re-checks the files that passed, with no `dep_inputs` entry needed.
#[test]
fn aspell_custom_conf_edit_rechecks() {
    require_tool("aspell");

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.checker.aspell]\nsrc_dirs = [\".\"]\nconf = \"custom.conf\"\n",
    )
    .unwrap();
    fs::write(project_path.join("custom.conf"), "lang en_US\n").unwrap();
    fs::write(project_path.join("doc.md"), "# Hello\n\nThis is correct.\n").unwrap();

    let first = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(
        first.status.success(),
        "first build should pass: stdout={}, stderr={}",
        String::from_utf8_lossy(&first.stdout),
        String::from_utf8_lossy(&first.stderr)
    );

    fs::write(
        project_path.join("custom.conf"),
        "lang en_US\nignore-case true\n",
    )
    .unwrap();

    let second =
        run_rsconstruct_with_env(project_path, &["build", "--verbose"], &[("NO_COLOR", "1")]);
    assert!(
        second.status.success(),
        "rebuild should pass: stdout={}, stderr={}",
        String::from_utf8_lossy(&second.stdout),
        String::from_utf8_lossy(&second.stderr)
    );
    let stdout = String::from_utf8_lossy(&second.stdout);
    assert!(
        stdout.contains("Processing:") && !stdout.contains("Skipping (unchanged):"),
        "editing the custom conf must re-run the check: {stdout}"
    );
}

/// A `conf` the user named that does not exist fails the build naming the
/// field and the file — never a silent run on aspell's defaults.
#[test]
fn aspell_missing_conf_fails() {
    require_tool("aspell");

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.checker.aspell]\nsrc_dirs = [\".\"]\nconf = \"conf/missing.conf\"\n",
    )
    .unwrap();
    fs::write(project_path.join("doc.md"), "# Hello\n\nThis is correct.\n").unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(
        !output.status.success(),
        "a missing conf must fail the build: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("aspell conf not found: conf/missing.conf"),
        "error must name the field and the file: {combined}"
    );
}
