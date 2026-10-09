use crate::common::run_rsconstruct_with_env;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

const CONFIG: &str = "[processor.checker.icpplint]\nsrc_dirs = [\"src\"]\n";

fn setup_project(config: &str, files: &[(&str, &str)]) -> TempDir {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    fs::write(temp_dir.path().join("rsconstruct.toml"), config)
        .expect("Failed to write rsconstruct.toml");
    // A repository root, so header guards are derived from the project path.
    fs::create_dir(temp_dir.path().join(".git")).expect("Failed to create .git");
    for (name, content) in files {
        let path = temp_dir.path().join(name);
        fs::create_dir_all(path.parent().unwrap()).expect("Failed to create dir");
        fs::write(&path, content).expect("Failed to write source file");
    }
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

const CLEAN_HEADER: &str = "// Copyright 2024 Example\n#ifndef SRC_FOO_H_\n#define SRC_FOO_H_\n\nclass Foo {\n public:\n  Foo();\n  int value() const;\n\n private:\n  int value_;\n};\n\n#endif  // SRC_FOO_H_\n";

const CLEAN_SOURCE: &str = "// Copyright 2024 Example\n#include \"src/foo.h\"\n\nFoo::Foo() : value_(0) {}\n\nint Foo::value() const {\n  return value_;\n}\n";

#[test]
fn icpplint_clean_files_pass() {
    let temp_dir = setup_project(
        CONFIG,
        &[("src/foo.h", CLEAN_HEADER), ("src/foo.cc", CLEAN_SOURCE)],
    );
    assert_passes(&build(temp_dir.path()), "clean C++ files");
}

#[test]
fn icpplint_reports_in_cpplint_format() {
    let source = "int main(){\n\tint x = (int)3.5;\n  return x;\n}\n";
    let temp_dir = setup_project(CONFIG, &[("src/main.cc", source)]);
    let output = build(temp_dir.path());
    assert_fails_with(
        &output,
        "a missing copyright",
        "src/main.cc:0:  No copyright message found.  You should have a line: \"Copyright [year] <Copyright Owner>\"  [legal/copyright] [5]",
    );
    assert_fails_with(
        &output,
        "a brace without a space",
        "src/main.cc:1:  Missing space before {  [whitespace/braces] [5]",
    );
    assert_fails_with(
        &output,
        "a tab",
        "src/main.cc:2:  Tab found; better to use spaces  [whitespace/tab] [1]",
    );
    assert_fails_with(
        &output,
        "a C-style cast",
        "src/main.cc:2:  Using C-style cast.  Use static_cast<int>(...) instead  [readability/casting] [4]",
    );
}

#[test]
fn icpplint_header_guard_uses_repository_path() {
    let header = "// Copyright 2024 Example\n#ifndef FOO_H_\n#define FOO_H_\n#endif  // FOO_H_\n";
    let temp_dir = setup_project(CONFIG, &[("src/foo.h", header)]);
    assert_fails_with(
        &build(temp_dir.path()),
        "a header guard without the directory",
        "src/foo.h:2:  #ifndef header guard has wrong style, please use: SRC_FOO_H_  [build/header_guard] [5]",
    );
}

#[test]
fn icpplint_reads_cpplint_cfg_and_filters_field() {
    let source = "// Copyright 2024 Example\nint main(){\n\tint x = (int)3.5;\n  return x;\n}\n";
    let cfg = "set noparent\nfilter=-whitespace/braces,-readability/casting\n";
    let temp_dir = setup_project(CONFIG, &[("src/main.cc", source), ("CPPLINT.cfg", cfg)]);
    let output = build(temp_dir.path());
    assert_fails_with(
        &output,
        "the tab the config leaves on",
        "src/main.cc:3:  Tab found; better to use spaces  [whitespace/tab] [1]",
    );
    assert!(
        !combined_output(&output).contains("whitespace/braces"),
        "CPPLINT.cfg must filter the brace finding: {}",
        combined_output(&output)
    );
    let config =
        "[processor.checker.icpplint]\nsrc_dirs = [\"src\"]\nfilters = [\"-whitespace/tab\"]\n";
    let temp_dir = setup_project(config, &[("src/main.cc", source), ("CPPLINT.cfg", cfg)]);
    assert_passes(&build(temp_dir.path()), "every finding filtered off");
}

#[test]
fn icpplint_named_config_file_and_nolint() {
    let source = "// Copyright 2024 Example\nint main(){  // NOLINT(whitespace/braces)\n  int x = (int)3.5;\n  return x;\n}\n";
    let config = "[processor.checker.icpplint]\nsrc_dirs = [\"src\"]\nconfig_file = \".cpplint\"\n";
    let temp_dir = setup_project(
        config,
        &[
            ("src/main.cc", source),
            (".cpplint", "filter=-readability/casting\n"),
        ],
    );
    assert_passes(
        &build(temp_dir.path()),
        "a NOLINT comment plus a named config file",
    );
}

#[test]
fn icpplint_line_length_field() {
    // The return line is 93 columns wide: over 80, under 120.
    let source = "// Copyright 2024 Example\nint main() {\n  return 1234567890 + 1234567890 + 1234567890 + 1234567890 + 1234567890 + 1234567890 + 12345;\n}\n";
    let temp_dir = setup_project(CONFIG, &[("src/main.cc", source)]);
    assert_fails_with(
        &build(temp_dir.path()),
        "a line over 80 columns",
        "src/main.cc:3:  Lines should be <= 80 characters long  [whitespace/line_length] [2]",
    );
    let config = "[processor.checker.icpplint]\nsrc_dirs = [\"src\"]\nline_length = 120\n";
    let temp_dir = setup_project(config, &[("src/main.cc", source)]);
    assert_passes(&build(temp_dir.path()), "the same line under a 120 limit");
}

#[test]
fn icpplint_rejects_bad_include_order() {
    let config =
        "[processor.checker.icpplint]\nsrc_dirs = [\"src\"]\ninclude_order = \"sideways\"\n";
    let temp_dir = setup_project(
        config,
        &[("src/foo.cc", CLEAN_SOURCE), ("src/foo.h", CLEAN_HEADER)],
    );
    assert_fails_with(
        &build(temp_dir.path()),
        "a bad include_order",
        "Invalid includeorder value sideways",
    );
}
