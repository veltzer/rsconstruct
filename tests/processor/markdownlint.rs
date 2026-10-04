use crate::common::{require_tool, run_rsconstruct_with_env};
use std::fs;
use tempfile::TempDir;

#[test]
fn markdownlint_valid_file() {
    require_tool("markdownlint");

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    // Point command to the system markdownlint, skip npm dependency
    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.markdownlint]\ncommand = \"markdownlint\"\nsrc_dirs = [\".\"]\n",
    )
    .unwrap();

    fs::write(
        project_path.join("doc.md"),
        "# Hello World\n\nThis is a test.\n",
    )
    .unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build", "-v"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "Build should succeed with valid markdown: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Processing:"),
        "Should process markdownlint: {}",
        stdout
    );
}

/// A src_dirs entry that does not exist is a config error naming the
/// processor and the entry, not a silent skip.
#[test]
fn markdownlint_nonexistent_src_dir_is_config_error() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.markdownlint]\nsrc_dirs = [\"mdlint_docs\"]\n",
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
        stderr.contains("processor.markdownlint") && stderr.contains("mdlint_docs"),
        "error must name the processor and the entry: {stderr}"
    );
}
