use crate::common::run_rsconstruct_with_env;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

const SCAN_ROOT: &str = "[processor.checker.iyamllint]\nsrc_dirs = [\"\"]\n";

/// A project whose root is scanned by iyamllint, holding one YAML file.
fn setup_project_with(config: &str, yaml: &str) -> TempDir {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    fs::write(temp_dir.path().join("rsconstruct.toml"), config)
        .expect("Failed to write rsconstruct.toml");
    fs::write(temp_dir.path().join("test.yaml"), yaml).expect("Failed to write test.yaml");
    temp_dir
}

fn setup_project(yaml: &str) -> TempDir {
    setup_project_with(SCAN_ROOT, yaml)
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
fn iyamllint_valid_file() {
    let temp_dir = setup_project("---\nname: test\nvalue: 42\n");
    assert_passes(&build(temp_dir.path()), "a valid file");
}

/// Several documents separated by `---` are valid YAML. The check used to
/// parse the whole file as one document and reject them with "more than one
/// document is not supported".
#[test]
fn iyamllint_multi_document_file() {
    let temp_dir = setup_project("---\nname: first\n---\nname: second\n...\n");
    assert_passes(&build(temp_dir.path()), "a multi-document file");
}

#[test]
fn iyamllint_invalid_file() {
    let temp_dir = setup_project("---\nname: [1, 2\n");
    assert_fails_with(
        &build(temp_dir.path()),
        "a file that does not parse",
        "syntax error",
    );
}

/// A bad second document fails the file: every document is parsed, not only
/// the first.
#[test]
fn iyamllint_invalid_later_document() {
    let temp_dir = setup_project("---\nname: first\n---\nname: [1, 2\n");
    assert_fails_with(
        &build(temp_dir.path()),
        "a bad second document",
        "syntax error",
    );
}

#[test]
fn iyamllint_duplicate_key() {
    let temp_dir = setup_project("---\nname: first\nname: second\n");
    assert_fails_with(
        &build(temp_dir.path()),
        "a duplicate mapping key",
        "duplication of key \"name\" in mapping (key-duplicates)",
    );
}

/// With no `.yamllint*` file, yamllint's `default` configuration applies:
/// trailing spaces and lines over 80 characters are errors.
#[test]
fn iyamllint_applies_yamllint_default_rules() {
    let temp_dir = setup_project("---\nkey: value   \n");
    assert_fails_with(
        &build(temp_dir.path()),
        "trailing spaces",
        "test.yaml:2:11: [error] trailing spaces (trailing-spaces)",
    );

    let long = format!("---\nkey: {}\n", "word ".repeat(20).trim_end());
    let temp_dir = setup_project(&long);
    assert_fails_with(
        &build(temp_dir.path()),
        "a long line",
        "line too long (104 > 80 characters) (line-length)",
    );

    let temp_dir = setup_project("---\na:\n  b:\n      c: 1\n");
    assert_fails_with(
        &build(temp_dir.path()),
        "inconsistent indentation",
        "wrong indentation: expected 4 but found 6 (indentation)",
    );
}

/// `.yamllint.yaml` in the project root is read, as yamllint reads it.
#[test]
fn iyamllint_honours_the_project_config() {
    let long = format!("key: {}\n", "word ".repeat(20).trim_end());
    let temp_dir = setup_project(&long);
    fs::write(
        temp_dir.path().join(".yamllint.yaml"),
        "extends: default\nrules:\n  document-start: disable\n  line-length:\n    max: 240\n",
    )
    .unwrap();
    assert_passes(
        &build(temp_dir.path()),
        "a long line under a config that allows it",
    );

    let temp_dir = setup_project("key: value   \n");
    fs::write(
        temp_dir.path().join(".yamllint.yaml"),
        "rules:\n  trailing-spaces: disable\n",
    )
    .unwrap();
    assert_passes(
        &build(temp_dir.path()),
        "trailing spaces with the rule disabled",
    );
}

/// `config_file` names the config explicitly, wherever it lives.
#[test]
fn iyamllint_config_file_field() {
    let temp_dir = setup_project_with(
        "[processor.checker.iyamllint]\nsrc_dirs = [\"\"]\nconfig_file = \"lint/yaml.yaml\"\n",
        "key: value   \n",
    );
    fs::create_dir_all(temp_dir.path().join("lint")).unwrap();
    fs::write(
        temp_dir.path().join("lint/yaml.yaml"),
        "rules:\n  trailing-spaces: disable\n",
    )
    .unwrap();
    assert_passes(&build(temp_dir.path()), "a config named by config_file");
}

/// Warnings (the default level of `comments`, `document-start`, `truthy`)
/// do not fail a file unless `strict` is set, like `yamllint --strict`.
#[test]
fn iyamllint_warnings_fail_only_when_strict() {
    let yaml = "#comment without a space\nkey: value\n";
    let temp_dir = setup_project(yaml);
    assert_passes(&build(temp_dir.path()), "a file with warnings only");

    let temp_dir = setup_project_with(
        "[processor.checker.iyamllint]\nsrc_dirs = [\"\"]\nstrict = true\n",
        yaml,
    );
    assert_fails_with(
        &build(temp_dir.path()),
        "a file with warnings under strict",
        "[warning] missing starting space in comment (comments)",
    );
}

/// `# yamllint disable-line` and friends are honoured.
#[test]
fn iyamllint_disable_directives() {
    let temp_dir = setup_project(
        "---\n# yamllint disable-line rule:trailing-spaces\nkey: value   \nother: 1\n",
    );
    assert_passes(&build(temp_dir.path()), "a line excused by a directive");

    let temp_dir = setup_project("# yamllint disable-file\nkey: value   \n");
    assert_passes(&build(temp_dir.path()), "a file excused by a directive");
}

/// A config yamllint would reject is a build failure, not a silent default.
#[test]
fn iyamllint_rejects_an_invalid_config() {
    let temp_dir = setup_project("---\nkey: value\n");
    fs::write(
        temp_dir.path().join(".yamllint.yaml"),
        "rules:\n  no-such-rule: enable\n",
    )
    .unwrap();
    assert_fails_with(
        &build(temp_dir.path()),
        "an unknown rule in the config",
        "no such rule: \"no-such-rule\"",
    );
}

/// The config's `ignore` patterns exclude files from linting.
#[test]
fn iyamllint_config_ignore_patterns() {
    let temp_dir = setup_project("---\nkey: value   \n");
    fs::write(
        temp_dir.path().join(".yamllint.yaml"),
        "extends: default\nignore: |\n  test.yaml\n",
    )
    .unwrap();
    assert_passes(&build(temp_dir.path()), "an ignored file");
}
