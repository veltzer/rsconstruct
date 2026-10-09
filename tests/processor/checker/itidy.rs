use crate::common::run_rsconstruct_with_env;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

const CONFIG: &str = "[processor.checker.itidy]\nsrc_dirs = [\"html\"]\n";

/// A project with one HTML file under `html/`.
fn setup_project_with(config: &str, html: &str) -> TempDir {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    fs::write(temp_dir.path().join("rsconstruct.toml"), config)
        .expect("Failed to write rsconstruct.toml");
    fs::create_dir_all(temp_dir.path().join("html")).unwrap();
    fs::write(temp_dir.path().join("html/page.html"), html).expect("Failed to write page.html");
    temp_dir
}

fn setup_project(html: &str) -> TempDir {
    setup_project_with(CONFIG, html)
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

const VALID: &str = "<!DOCTYPE html>\n<html>\n<head><title>Page</title></head>\n<body><p>hello</p></body>\n</html>\n";

#[test]
fn itidy_valid_document() {
    let temp_dir = setup_project(VALID);
    assert_passes(&build(temp_dir.path()), "a valid document");
}

/// Warnings fail the product, like `tidy -errors` exiting 1, and are
/// reported in tidy's format with the file in front.
#[test]
fn itidy_reports_in_tidy_format() {
    let temp_dir = setup_project("<p>hello</p>\n");
    let output = build(temp_dir.path());
    assert_fails_with(
        &output,
        "a document without doctype",
        "html/page.html: line 1 column 1 - Warning: missing <!DOCTYPE> declaration",
    );
    let text = combined_output(&output);
    assert!(
        text.contains("line 1 column 1 - Warning: inserting implicit <body>"),
        "{text}"
    );
    assert!(
        text.contains("line 1 column 1 - Warning: inserting missing 'title' element"),
        "{text}"
    );
}

#[test]
fn itidy_unknown_tag_is_an_error() {
    let temp_dir = setup_project(
        "<!DOCTYPE html>\n<html><head><title>t</title></head>\n<body><md-button>x</md-button></body></html>\n",
    );
    assert_fails_with(
        &build(temp_dir.path()),
        "an unknown tag",
        "line 3 column 7 - Error: <md-button> is not recognized! Did you mean to enable the custom-tags option?",
    );
}

/// `options` are tidy's `--name value` settings; `custom-tags: blocklevel`
/// makes `<md-button>` a known block.
#[test]
fn itidy_options_field() {
    let temp_dir = setup_project_with(
        "[processor.checker.itidy]\nsrc_dirs = [\"html\"]\noptions = [\"custom-tags: blocklevel\"]\n",
        "<!DOCTYPE html>\n<html><head><title>t</title></head>\n<body><md-button>x</md-button></body></html>\n",
    );
    assert_passes(
        &build(temp_dir.path()),
        "a custom tag with custom-tags enabled",
    );
}

/// `config_file` is read like `tidy -config`.
#[test]
fn itidy_config_file_field() {
    let temp_dir = setup_project_with(
        "[processor.checker.itidy]\nsrc_dirs = [\"html\"]\nconfig_file = \"tidy.conf\"\n",
        "<!DOCTYPE html>\n<html><head><title>t</title></head>\n<body><p></p><md-list>x</md-list></body></html>\n",
    );
    fs::write(
        temp_dir.path().join("tidy.conf"),
        "# keep empty paragraphs, know the material tags\ndrop-empty-elements: no\nnew-blocklevel-tags: md-list, md-list-item\n",
    )
    .unwrap();
    assert_passes(
        &build(temp_dir.path()),
        "a document allowed by the config file",
    );
}

/// An unknown option in the config file is a build error, not a warning
/// tidy prints and ignores.
#[test]
fn itidy_rejects_unknown_options() {
    let temp_dir = setup_project_with(
        "[processor.checker.itidy]\nsrc_dirs = [\"html\"]\noptions = [\"no-such-option: yes\"]\n",
        VALID,
    );
    assert_fails_with(
        &build(temp_dir.path()),
        "an unknown option",
        "unknown option: no-such-option",
    );
}

/// Only UTF-8 input is read; a UTF-16 file fails with a clear reason.
#[test]
fn itidy_refuses_utf16_input() {
    let temp_dir = setup_project("");
    let mut utf16 = vec![0xFF, 0xFE];
    for unit in "<p>x</p>".encode_utf16() {
        utf16.extend_from_slice(&unit.to_le_bytes());
    }
    fs::write(temp_dir.path().join("html/page.html"), utf16).unwrap();
    assert_fails_with(
        &build(temp_dir.path()),
        "a UTF-16 file",
        "UTF-16 input (byte order mark found) is not supported",
    );
}
