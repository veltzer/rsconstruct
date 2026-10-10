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

/// The engine's own fixtures (`src/engines/shellcheck/testdata`), which its
/// unit tests check against shellcheck under each engine option.
fn engine_fixtures() -> Vec<(String, Vec<u8>)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/engines/shellcheck/testdata");
    let mut files: Vec<(String, Vec<u8>)> = fs::read_dir(&dir)
        .expect("read the engine's testdata")
        .map(|e| {
            let path = e.expect("testdata entry").path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            (name, fs::read(&path).expect("read a fixture"))
        })
        .collect();
    files.sort();
    files
}

/// The findings in a build's output: the gcc-format lines of the failed
/// products as they fail, on stderr (the summary on stdout repeats them).
fn reported_findings(output: &std::process::Output) -> Vec<String> {
    let text = String::from_utf8_lossy(&output.stderr);
    let mut lines: Vec<String> = text
        .lines()
        .map(|l| match l.split_once("[processor.checker.ishellcheck] ") {
            Some((_, rest)) => rest,
            None => l,
        })
        .filter(|l| {
            l.starts_with("src/") && l.contains(": ") && l.ends_with(']') && l.contains(" [SC")
        })
        .map(str::to_string)
        .collect();
    lines.sort();
    lines
}

/// `shellcheck --format=gcc ARGS FILE` for each file on its own, as each
/// file is its own product: a file is never an input of another's check.
fn shellcheck_findings(project: &Path, args: &[&str], files: &[String]) -> Vec<String> {
    let mut lines = Vec::new();
    for file in files {
        let out = std::process::Command::new("shellcheck")
            .current_dir(project)
            .arg("--format=gcc")
            .args(args)
            .arg(file)
            .output()
            .expect("run shellcheck");
        lines.extend(
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .map(str::to_string),
        );
    }
    lines.sort();
    lines
}

/// Each `ishellcheck` field reaches the engine as the shellcheck option of
/// the same name does: the processor's findings over the engine's fixtures
/// are shellcheck's with the matching command line.
#[test]
fn ishellcheck_fields_match_shellcheck_options() {
    crate::common::require_tool("shellcheck");
    let fixtures = engine_fixtures();
    let cases: &[(&str, &[&str])] = &[
        ("norc = true", &["--norc"]),
        ("norc = true\nshell = \"sh\"", &["--norc", "--shell=sh"]),
        (
            "norc = true\nseverity = \"warning\"",
            &["--norc", "--severity=warning"],
        ),
        (
            "norc = true\nexclude = [\"SC2086\", \"2034\"]",
            &["--norc", "--exclude=SC2086,SC2034"],
        ),
        (
            "norc = true\ninclude = [\"SC2154\"]",
            &["--norc", "--include=SC2154"],
        ),
        (
            "norc = true\nenable = [\"all\"]",
            &["--norc", "--enable=all"],
        ),
        (
            "norc = true\nexternal_sources = true\ncheck_sourced = true\nsource_path = [\"SCRIPTDIR\"]",
            &[
                "--norc",
                "--external-sources",
                "--check-sourced",
                "--source-path=SCRIPTDIR",
            ],
        ),
        (
            "norc = true\nextended_analysis = \"false\"",
            &["--norc", "--extended-analysis=false"],
        ),
        ("rcfile = \"checkrc\"", &["--rcfile=checkrc"]),
    ];
    for (fields, args) in cases {
        let config = format!(
            "[processor.checker.ishellcheck]\nsrc_dirs = [\"src\"]\nsrc_extensions = [\".sh\", \".bash\", \".ksh\"]\n{fields}\n"
        );
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let project = temp_dir.path();
        fs::write(project.join("rsconstruct.toml"), &config).expect("write rsconstruct.toml");
        fs::write(
            project.join("checkrc"),
            "disable=SC2086\nenable=require-variable-braces\n",
        )
        .expect("write the rcfile");
        fs::create_dir(project.join("src")).expect("create src");
        let mut names = Vec::new();
        for (name, contents) in &fixtures {
            fs::write(project.join("src").join(name), contents).expect("write a fixture");
            names.push(format!("src/{name}"));
        }
        let output = run_rsconstruct_with_env(project, &["build", "-k"], &[("NO_COLOR", "1")]);
        let expected = shellcheck_findings(project, args, &names);
        assert!(!expected.is_empty(), "{fields}: the fixtures have findings");
        assert_eq!(
            reported_findings(&output),
            expected,
            "{fields} against shellcheck {args:?}: ishellcheck (left) and shellcheck (right)"
        );
    }
}
