use crate::common::run_rsconstruct_with_env;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

const CONFIG: &str = "[processor.checker.isvglint]\nsrc_dirs = [\"img\"]\n";

/// A project with one SVG file under `img/`.
fn setup_project_with(config: &str, svg: &str) -> TempDir {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    fs::write(temp_dir.path().join("rsconstruct.toml"), config)
        .expect("Failed to write rsconstruct.toml");
    fs::create_dir_all(temp_dir.path().join("img")).unwrap();
    fs::write(temp_dir.path().join("img/icon.svg"), svg).expect("Failed to write icon.svg");
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

const VALID: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" role=\"img\" viewBox=\"0 0 24 24\">\n  <title>t</title>\n  <path d=\"M1 1\" fill=\"red\"/>\n</svg>\n";

#[test]
fn isvglint_valid_document() {
    let temp_dir = setup_project_with(CONFIG, VALID);
    assert_passes(&build(temp_dir.path()), "a well-formed SVG");
}

/// With no rules configured, only `valid` runs, as in svglint; its message
/// is fast-xml-parser's.
#[test]
fn isvglint_reports_malformed_xml() {
    let temp_dir = setup_project_with(CONFIG, "<svg>\n<g>\n<path d=\"M1 1\"/>\n</svg>\n");
    assert_fails_with(
        &build(temp_dir.path()),
        "a mismatched closing tag",
        "img/icon.svg: valid: Expected closing tag 'g' (opened in line 2, col 1) instead of closing tag 'svg'.",
    );
}

#[test]
fn isvglint_elm_rule() {
    let config = "[processor.checker.isvglint]\nsrc_dirs = [\"img\"]\n[processor.checker.isvglint.elm]\n\"svg > title\" = 1\n\"rect\" = false\n";
    let svg =
        "<svg xmlns=\"http://www.w3.org/2000/svg\">\n  <rect width=\"1\" height=\"1\"/>\n</svg>\n";
    let temp_dir = setup_project_with(config, svg);
    let output = build(temp_dir.path());
    assert_fails_with(
        &output,
        "a forbidden element",
        "img/icon.svg:2:3: elm: Element disallowed (<rect>)",
    );
    assert_fails_with(
        &output,
        "a missing element",
        "img/icon.svg: elm: Found 0 elements for 'svg > title', expected 1",
    );
}

#[test]
fn isvglint_attr_rule() {
    let config = "[processor.checker.isvglint]\nsrc_dirs = [\"img\"]\n[[processor.checker.isvglint.attr]]\n\"rule::selector\" = \"svg\"\n\"rule::whitelist\" = true\nxmlns = \"http://www.w3.org/2000/svg\"\nviewBox = { regex = \"^0 0 \\\\d+ \\\\d+$\" }\n";
    let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24\" width=\"24\"/>\n";
    let temp_dir = setup_project_with(config, svg);
    let output = build(temp_dir.path());
    assert_fails_with(
        &output,
        "a value not matching its pattern",
        "img/icon.svg:1:1: attr-1: Expected attribute 'viewBox' to match /^0 0 \\d+ \\d+$/, was \"0 0 24\" (<svg>)",
    );
    assert_fails_with(
        &output,
        "an attribute outside the whitelist",
        "Found extra attributes [\"width\"] with whitelisting enabled",
    );
}

#[test]
fn isvglint_valid_can_be_disabled() {
    let config = "[processor.checker.isvglint]\nsrc_dirs = [\"img\"]\nvalid = false\n";
    let temp_dir = setup_project_with(config, "<svg><g></svg>\n");
    assert_passes(
        &build(temp_dir.path()),
        "malformed XML with the valid rule off",
    );
}

#[test]
fn isvglint_rejects_bad_rules() {
    let config = "[processor.checker.isvglint]\nsrc_dirs = [\"img\"]\n[processor.checker.isvglint.elm]\n\"svg:root\" = true\n";
    let temp_dir = setup_project_with(config, VALID);
    assert_fails_with(
        &build(temp_dir.path()),
        "an unsupported selector",
        "unsupported pseudo-class :root",
    );
}
