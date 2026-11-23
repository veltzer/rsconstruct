use crate::common::{require_tool, run_rsconstruct_with_env};
use std::fs;
use tempfile::TempDir;

#[test]
fn make_valid_makefile() {
    require_tool("make");

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    // Put the Makefile in a subdirectory so .rsconstruct/ cache files
    // (which are sibling-scanned by the make processor) don't
    // cause spurious rebuilds.
    fs::create_dir_all(project_path.join("proj")).unwrap();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.checker.make]\nsrc_dirs = [\".\"]\n",
    )
    .unwrap();

    fs::write(
        project_path.join("proj/Makefile"),
        ".PHONY: all\nall:\n\t@echo \"hello from make\"\n",
    )
    .unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build", "-v"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "Build should succeed with valid Makefile: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Processing:"),
        "Should process make: {stdout}"
    );
}

#[test]
fn make_incremental_skip() {
    require_tool("make");

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::create_dir_all(project_path.join("proj")).unwrap();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.checker.make]\nsrc_dirs = [\".\"]\n",
    )
    .unwrap();

    fs::write(
        project_path.join("proj/Makefile"),
        ".PHONY: all\nall:\n\t@echo \"hello from make\"\n",
    )
    .unwrap();

    // First build
    let output1 = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output1.status.success());

    // Second build should skip
    let output2 =
        run_rsconstruct_with_env(project_path, &["build", "--verbose"], &[("NO_COLOR", "1")]);
    assert!(output2.status.success());
    let stdout2 = String::from_utf8_lossy(&output2.stdout);
    assert!(
        stdout2.contains("[processor.checker.make] Skipping (unchanged):"),
        "Second build should skip: {stdout2}"
    );
}

/// Write a make project whose stanza declares `required_tools = [tool]`.
fn make_project_requiring(tool: &str) -> TempDir {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();
    fs::create_dir_all(project_path.join("proj")).unwrap();
    fs::write(
        project_path.join("rsconstruct.toml"),
        format!("[processor.checker.make]\nsrc_dirs = [\".\"]\nrequired_tools = [\"{tool}\"]\n"),
    )
    .unwrap();
    fs::write(
        project_path.join("proj/Makefile"),
        ".PHONY: all\nall:\n\t@echo \"hello from make\"\n",
    )
    .unwrap();
    temp_dir
}

/// `required_tools` is honoured by the hand-written make checker: a declared
/// tool that is not on PATH fails the build with the missing-tool error,
/// before anything runs.
#[test]
fn make_required_tools_missing_fails_build() {
    require_tool("make");
    let temp_dir = make_project_requiring("rsconstruct-test-missing-tool");
    let output = run_rsconstruct_with_env(temp_dir.path(), &["build"], &[("NO_COLOR", "1")]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "build must fail: stderr={stderr}");
    assert!(
        stderr.contains("Missing required tools")
            && stderr.contains("rsconstruct-test-missing-tool (needed by: processor.checker.make)"),
        "expected the missing-tool error: stderr={stderr}"
    );
}

/// A declared tool that is on PATH leaves the build unaffected.
#[test]
fn make_required_tools_present_builds() {
    require_tool("make");
    let temp_dir = make_project_requiring("sh");
    let output = run_rsconstruct_with_env(temp_dir.path(), &["build"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "build should succeed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Every file under the Makefile's directory is an input, whatever its
/// name: editing a source next to the Makefile re-runs make. The sibling
/// filter used to match no file at all, leaving the Makefile as the only
/// input.
#[test]
fn make_reruns_when_a_file_beside_the_makefile_changes() {
    require_tool("make");

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();
    fs::create_dir_all(project_path.join("proj/src")).unwrap();
    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.checker.make]\nsrc_dirs = [\".\"]\n",
    )
    .unwrap();
    fs::write(
        project_path.join("proj/Makefile"),
        ".PHONY: all\nall:\n\t@cat src/data\n",
    )
    .unwrap();
    fs::write(project_path.join("proj/src/data"), "one\n").unwrap();

    let first = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );

    fs::write(project_path.join("proj/src/data"), "two\n").unwrap();
    let second =
        run_rsconstruct_with_env(project_path, &["build", "--verbose"], &[("NO_COLOR", "1")]);
    assert!(second.status.success());
    let stdout = String::from_utf8_lossy(&second.stdout);
    assert!(
        !stdout.contains("[processor.checker.make] Skipping (unchanged):"),
        "make must re-run after a sibling file changed: {stdout}"
    );
}
