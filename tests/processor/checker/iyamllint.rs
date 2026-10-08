use crate::common::run_rsconstruct_with_env;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

/// A project whose root is scanned by iyamllint, holding one YAML file.
fn setup_project(yaml: &str) -> TempDir {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    fs::write(
        temp_dir.path().join("rsconstruct.toml"),
        "[processor.checker.iyamllint]\nsrc_dirs = [\"\"]\n",
    )
    .expect("Failed to write rsconstruct.toml");
    fs::write(temp_dir.path().join("test.yaml"), yaml).expect("Failed to write test.yaml");
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

#[test]
fn iyamllint_valid_file() {
    let temp_dir = setup_project("name: test\nvalue: 42\n");
    let output = build(temp_dir.path());
    assert!(
        output.status.success(),
        "a valid file must pass: {}",
        combined_output(&output)
    );
}

/// Several documents separated by `---` are valid YAML. The check used to
/// parse the whole file as one document and reject them with "more than one
/// document is not supported".
#[test]
fn iyamllint_multi_document_file() {
    let temp_dir = setup_project("---\nname: first\n---\nname: second\n...\n");
    let output = build(temp_dir.path());
    assert!(
        output.status.success(),
        "a multi-document file must pass: {}",
        combined_output(&output)
    );
}

#[test]
fn iyamllint_invalid_file() {
    let temp_dir = setup_project("name: [1, 2\n");
    let output = build(temp_dir.path());
    assert!(
        !output.status.success(),
        "a file that does not parse must fail"
    );
    let text = combined_output(&output);
    assert!(
        text.contains("Invalid YAML"),
        "no parse error reported: {text}"
    );
}

/// A bad second document fails the file: every document is parsed, not only
/// the first.
#[test]
fn iyamllint_invalid_later_document() {
    let temp_dir = setup_project("---\nname: first\n---\nname: [1, 2\n");
    let output = build(temp_dir.path());
    assert!(
        !output.status.success(),
        "a bad second document must fail the file"
    );
    let text = combined_output(&output);
    assert!(
        text.contains("Invalid YAML"),
        "no parse error reported: {text}"
    );
}

#[test]
fn iyamllint_duplicate_key() {
    let temp_dir = setup_project("name: first\nname: second\n");
    let output = build(temp_dir.path());
    assert!(
        !output.status.success(),
        "a duplicate mapping key must fail"
    );
    let text = combined_output(&output);
    assert!(
        text.contains("duplicate"),
        "no duplicate-key error reported: {text}"
    );
}
