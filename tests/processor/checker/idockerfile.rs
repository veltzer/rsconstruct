use crate::common::run_rsconstruct_with_env;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

const CONFIG: &str = "[processor.checker.idockerfile]\nsrc_files = [\"Dockerfile\"]\n";

fn setup_project_with(config: &str, dockerfile: &str) -> TempDir {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    fs::write(temp_dir.path().join("rsconstruct.toml"), config)
        .expect("Failed to write rsconstruct.toml");
    fs::write(temp_dir.path().join("Dockerfile"), dockerfile).expect("Failed to write Dockerfile");
    temp_dir
}

fn build(project_path: &Path) -> std::process::Output {
    run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")])
}

fn combined_output(output: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn assert_passes(output: &std::process::Output, what: &str) {
    assert!(
        output.status.success(),
        "{what} must pass: {}",
        combined_output(output)
    );
}

fn assert_fails_with(output: &std::process::Output, what: &str, needle: &str) {
    assert!(!output.status.success(), "{what} must fail");
    let text = combined_output(output);
    assert!(
        text.contains(needle),
        "{what}: {needle:?} not reported: {text}"
    );
}

const VALID: &str = "FROM ubuntu:24.04\nRUN apt-get update && apt-get install -y --no-install-recommends curl=8.5.0-2 \\\n    && rm -rf /var/lib/apt/lists/*\nCMD [\"curl\", \"--version\"]\n";

#[test]
fn idockerfile_valid_dockerfile() {
    let temp_dir = setup_project_with(CONFIG, VALID);
    assert_passes(&build(temp_dir.path()), "a clean Dockerfile");
}

#[test]
fn idockerfile_reports_in_hadolint_format() {
    let temp_dir = setup_project_with(CONFIG, "FROM ubuntu\nRUN apt-get install foo\n");
    let output = build(temp_dir.path());
    assert_fails_with(
        &output,
        "an untagged image",
        "Dockerfile:1 DL3006 warning: Always tag the version of an image explicitly",
    );
    assert_fails_with(
        &output,
        "an unpinned apt-get install",
        "Dockerfile:2 DL3008 warning: Pin versions in apt get install.",
    );
}

#[test]
fn idockerfile_reads_hadolint_yaml() {
    let temp_dir = setup_project_with(CONFIG, "FROM ubuntu\nRUN apt-get install foo\n");
    fs::write(
        temp_dir.path().join(".hadolint.yaml"),
        "ignored:\n  - DL3006\n  - DL3008\n  - DL3014\n  - DL3015\n",
    )
    .unwrap();
    assert_passes(
        &build(temp_dir.path()),
        "all findings ignored by .hadolint.yaml",
    );
}

#[test]
fn idockerfile_ignored_field_and_threshold() {
    let config = "[processor.checker.idockerfile]\nsrc_files = [\"Dockerfile\"]\nignored = [\"DL3006\", \"DL3008\", \"DL3014\"]\nfailure_threshold = \"warning\"\n";
    // Only DL3015 (info) remains, below the warning threshold: passes.
    let temp_dir = setup_project_with(config, "FROM ubuntu\nRUN apt-get install foo\n");
    assert_passes(
        &build(temp_dir.path()),
        "an info finding under a warning threshold",
    );
}

#[test]
fn idockerfile_inline_pragma() {
    let temp_dir = setup_project_with(
        CONFIG,
        "FROM ubuntu:24.04\n# hadolint ignore=DL3008,DL3014,DL3015\nRUN apt-get install foo\n",
    );
    assert_passes(
        &build(temp_dir.path()),
        "findings silenced by an inline pragma",
    );
}

#[test]
fn idockerfile_parse_error() {
    let temp_dir = setup_project_with(CONFIG, "FROM ubuntu:24.04\nBOGUS instruction\n");
    assert_fails_with(
        &build(temp_dir.path()),
        "an unknown instruction",
        "Dockerfile:2:1 unexpected",
    );
}

#[test]
fn idockerfile_rejects_bad_threshold() {
    let config = "[processor.checker.idockerfile]\nsrc_files = [\"Dockerfile\"]\nfailure_threshold = \"loud\"\n";
    let temp_dir = setup_project_with(config, VALID);
    assert_fails_with(
        &build(temp_dir.path()),
        "a bad threshold",
        "Invalid severity: loud",
    );
}
