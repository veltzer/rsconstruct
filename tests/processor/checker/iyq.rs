use crate::common::run_rsconstruct_with_env;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

const CONFIG: &str = "[processor.checker.iyq]\nsrc_dirs = [\"data\"]\n";

fn setup_project_with(config: &str, yaml: &str) -> TempDir {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    fs::write(temp_dir.path().join("rsconstruct.toml"), config)
        .expect("Failed to write rsconstruct.toml");
    fs::create_dir_all(temp_dir.path().join("data")).unwrap();
    fs::write(temp_dir.path().join("data/test.yaml"), yaml).expect("Failed to write test.yaml");
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

#[test]
fn iyq_valid_yaml_passes() {
    let temp_dir = setup_project_with(CONFIG, "---\nname: test\nvalue: 42\nlist:\n  - a\n  - b\n");
    assert_passes(
        &build(temp_dir.path()),
        "valid YAML under the default filter",
    );
}

#[test]
fn iyq_multiple_documents() {
    let temp_dir = setup_project_with(CONFIG, "a: 1\n---\nb: 2\n---\n- 3\n");
    assert_passes(&build(temp_dir.path()), "a multi-document file");
}

#[test]
fn iyq_invalid_yaml_fails() {
    let temp_dir = setup_project_with(CONFIG, "name: [unclosed\n");
    assert_fails_with(&build(temp_dir.path()), "invalid YAML", "data/test.yaml");
}

#[test]
fn iyq_filter_errors_fail() {
    let config =
        "[processor.checker.iyq]\nsrc_dirs = [\"data\"]\nfilter = \".name | ascii_downcase\"\n";
    let temp_dir = setup_project_with(config, "name: 42\n");
    assert_fails_with(
        &build(temp_dir.path()),
        "a filter error",
        "data/test.yaml: document 1: jq:",
    );
}

#[test]
fn iyq_filter_runs_over_each_document() {
    let config = "[processor.checker.iyq]\nsrc_dirs = [\"data\"]\nfilter = \"if has(\\\"name\\\") then . else error(\\\"no name\\\") end\"\n";
    let temp_dir = setup_project_with(config, "name: a\n---\nother: b\n");
    assert_fails_with(
        &build(temp_dir.path()),
        "a document rejected by the filter",
        "data/test.yaml: document 2: jq: \"no name\"",
    );
}

#[test]
fn iyq_rejects_a_bad_filter() {
    let config = "[processor.checker.iyq]\nsrc_dirs = [\"data\"]\nfilter = \".[ \"\n";
    let temp_dir = setup_project_with(config, "a: 1\n");
    assert_fails_with(
        &build(temp_dir.path()),
        "a filter that does not parse",
        "iyq filter \".[ \"",
    );
}
