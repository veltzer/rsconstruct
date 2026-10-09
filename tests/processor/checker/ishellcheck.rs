use crate::common::run_rsconstruct_with_env;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

const CONFIG: &str = "[processor.checker.ishellcheck]\nsrc_dirs = [\"src\"]\n";

fn setup_project(config: &str, files: &[(&str, &str)]) -> TempDir {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    fs::write(temp_dir.path().join("rsconstruct.toml"), config)
        .expect("Failed to write rsconstruct.toml");
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

#[test]
fn ishellcheck_clean_file_passes() {
    let temp_dir = setup_project(CONFIG, &[("src/ok.sh", "#!/bin/sh\necho \"ok\"\n")]);
    assert_passes(&build(temp_dir.path()), "a clean shell script");
}

#[test]
fn ishellcheck_reports_in_gcc_format() {
    let temp_dir = setup_project(CONFIG, &[("src/bad.sh", "#!/bin/bash\nfoo=1\necho $1\n")]);
    let output = build(temp_dir.path());
    assert_fails_with(
        &output,
        "an unused variable",
        "src/bad.sh:2:1: warning: foo appears unused. Verify use (or export if used externally). [SC2034]",
    );
    assert_fails_with(
        &output,
        "an unquoted expansion",
        "src/bad.sh:3:6: note: Double quote to prevent globbing and word splitting. [SC2086]",
    );
}

/// The dialect comes from the shebang: `function` is a bashism in `sh`.
#[test]
fn ishellcheck_checks_the_shebang_dialect() {
    let temp_dir = setup_project(CONFIG, &[("src/f.sh", "#!/bin/sh\nfunction f { :; }\n")]);
    assert_fails_with(
        &build(temp_dir.path()),
        "a bashism in sh",
        "src/f.sh:2:1: warning: 'function' keyword is non-standard. Use 'foo()' instead of 'function foo'. [SC2113]",
    );
}

#[test]
fn ishellcheck_honours_directives_and_shellcheckrc() {
    let temp_dir = setup_project(
        CONFIG,
        &[
            ("src/d.sh", "#!/bin/sh\n# shellcheck disable=SC2034\ny=1\n"),
            ("src/r.sh", "#!/bin/bash\necho $1\n"),
            (".shellcheckrc", "disable=SC2086\n"),
        ],
    );
    assert_passes(
        &build(temp_dir.path()),
        "findings silenced by directive and rc file",
    );
}

#[test]
fn ishellcheck_exclude_and_severity() {
    let config = "[processor.checker.ishellcheck]\nsrc_dirs = [\"src\"]\nexclude = [\"SC2034\"]\nseverity = \"warning\"\n";
    let temp_dir = setup_project(config, &[("src/bad.sh", "#!/bin/bash\nfoo=1\necho $1\n")]);
    assert_passes(
        &build(temp_dir.path()),
        "an excluded warning and a note below the severity",
    );
}

#[test]
fn ishellcheck_rejects_an_unknown_shell() {
    let config = "[processor.checker.ishellcheck]\nsrc_dirs = [\"src\"]\nshell = \"fish\"\n";
    let temp_dir = setup_project(config, &[("src/ok.sh", "#!/bin/sh\necho \"ok\"\n")]);
    assert_fails_with(
        &build(temp_dir.path()),
        "an unknown shell",
        "ishellcheck: unknown shell \"fish\"",
    );
}
