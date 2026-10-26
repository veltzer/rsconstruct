use crate::common::run_rsconstruct_with_env;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

const CONFIG: &str = "[processor.checker.icheckstyle]\nsrc_dirs = [\"src\"]\n";

/// The fleet's `checkstyle.xml`: every check icheckstyle implements.
const CHECKSTYLE_XML: &str = r#"<?xml version="1.0"?>
<!DOCTYPE module PUBLIC "-//Checkstyle//DTD Checkstyle Configuration 1.3//EN" "https://checkstyle.org/dtds/configuration_1_3.dtd">
<module name="Checker">
  <module name="NewlineAtEndOfFile"/>
  <module name="TreeWalker">
    <module name="ArrayTypeStyle"/>
    <module name="CovariantEquals"/>
    <module name="DefaultComesLast"/>
    <module name="EmptyStatement"/>
    <module name="EqualsHashCode"/>
    <module name="FallThrough"/>
    <module name="ModifiedControlVariable"/>
    <module name="OneStatementPerLine"/>
    <module name="RedundantImport"/>
    <module name="SimplifyBooleanExpression"/>
    <module name="SimplifyBooleanReturn"/>
    <module name="StringLiteralEquality"/>
    <module name="UnusedImports"/>
    <module name="UpperEll"/>
  </module>
</module>
"#;

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
fn icheckstyle_clean_file_passes() {
    let temp_dir = setup_project(
        CONFIG,
        &[
            ("checkstyle.xml", CHECKSTYLE_XML),
            ("src/Ok.java", "class Ok {\n    int x = 1;\n}\n"),
        ],
    );
    assert_passes(&build(temp_dir.path()), "a clean Java file");
}

#[test]
fn icheckstyle_reports_in_checkstyle_format() {
    let temp_dir = setup_project(
        CONFIG,
        &[
            ("checkstyle.xml", CHECKSTYLE_XML),
            (
                "src/Bad.java",
                "import java.util.List;\nclass Bad {\n    long x = 1l;\n}",
            ),
        ],
    );
    let output = build(temp_dir.path());
    assert_fails_with(
        &output,
        "a missing newline",
        "[ERROR] src/Bad.java:1: File does not end with a newline. [NewlineAtEndOfFile]",
    );
    assert_fails_with(
        &output,
        "an unused import",
        "[ERROR] src/Bad.java:1:8: Unused import - java.util.List. [UnusedImports]",
    );
    assert_fails_with(
        &output,
        "a lower case ell",
        "[ERROR] src/Bad.java:3:14: Should use uppercase 'L'. [UpperEll]",
    );
}

#[test]
fn icheckstyle_reports_parse_errors() {
    let temp_dir = setup_project(
        CONFIG,
        &[
            ("checkstyle.xml", CHECKSTYLE_XML),
            ("src/Broken.java", "class Broken {\n    void f( {\n}\n"),
        ],
    );
    assert_fails_with(
        &build(temp_dir.path()),
        "a syntax error",
        "src/Broken.java:2:13: unexpected token: {",
    );
}

#[test]
fn icheckstyle_warnings_do_not_fail() {
    let config = CHECKSTYLE_XML.replace(
        "<module name=\"Checker\">",
        "<module name=\"Checker\">\n  <property name=\"severity\" value=\"warning\"/>",
    );
    let temp_dir = setup_project(
        CONFIG,
        &[
            ("checkstyle.xml", &config),
            ("src/Warn.java", "class Warn {\n    long x = 1l;\n}\n"),
        ],
    );
    let output = build(temp_dir.path());
    assert_passes(&output, "a file with warnings only");
    let text = combined_output(&output);
    assert!(
        text.contains("[WARN] src/Warn.java:2:14: Should use uppercase 'L'. [UpperEll]"),
        "the warning is printed: {text}"
    );
}

#[test]
fn icheckstyle_reads_config_file() {
    let config =
        "[processor.checker.icheckstyle]\nsrc_dirs = [\"src\"]\nconfig_file = \"conf/rules.xml\"\n";
    let rules = "<module name=\"Checker\"><module name=\"TreeWalker\"><module name=\"EmptyStatement\"/></module></module>\n";
    let temp_dir = setup_project(
        config,
        &[
            ("conf/rules.xml", rules),
            ("src/E.java", "class E {\n    void f() { ; }\n}\n"),
        ],
    );
    assert_fails_with(
        &build(temp_dir.path()),
        "an empty statement",
        "[ERROR] src/E.java:2:16: Empty statement. [EmptyStatement]",
    );
}

#[test]
fn icheckstyle_rejects_unknown_modules() {
    let rules = "<module name=\"Checker\"><module name=\"TreeWalker\"><module name=\"MagicNumber\"/></module></module>\n";
    let temp_dir = setup_project(
        CONFIG,
        &[
            ("checkstyle.xml", rules),
            ("src/Ok.java", "class Ok {\n}\n"),
        ],
    );
    assert_fails_with(
        &build(temp_dir.path()),
        "an unimplemented module",
        "icheckstyle does not implement module 'MagicNumber'",
    );
}

#[test]
fn icheckstyle_requires_a_config_file() {
    let temp_dir = setup_project(CONFIG, &[("src/Ok.java", "class Ok {\n}\n")]);
    assert_fails_with(
        &build(temp_dir.path()),
        "a missing checkstyle.xml",
        "Failed to read checkstyle configuration checkstyle.xml",
    );
}

/// The engine's fixtures (`src/engines/checkstyle/testdata`).
fn engine_fixtures() -> Vec<(String, Vec<u8>)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/engines/checkstyle/testdata");
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

/// The message lines of a build's failed products, on stderr.
fn reported_messages(output: &std::process::Output) -> Vec<String> {
    let text = String::from_utf8_lossy(&output.stderr);
    let mut lines: Vec<String> = text
        .lines()
        .map(|l| match l.split_once("[processor.checker.icheckstyle] ") {
            Some((_, rest)) => rest,
            None => l,
        })
        // The reporter names the product before the message (`src/A.java:
        // [ERROR] src/A.java:1: ...`); the message line itself names the file.
        .map(|l| l.find("[ERROR] src/").map_or(l, |at| &l[at..]))
        .filter(|l| l.starts_with("[ERROR] src/"))
        .map(str::to_string)
        .collect();
    lines.sort();
    lines.dedup();
    lines
}

/// `checkstyle -c checkstyle.xml FILE` for each file that parses, its
/// message lines with the project directory taken off the paths.
fn checkstyle_messages(project: &Path, files: &[String]) -> Vec<String> {
    let prefix = format!("{}/", project.canonicalize().expect("canonical").display());
    let mut lines = Vec::new();
    for file in files {
        let out = std::process::Command::new("checkstyle")
            .current_dir(project)
            .args(["-c", "checkstyle.xml"])
            .arg(file)
            .output()
            .expect("run checkstyle");
        let stdout = String::from_utf8_lossy(&out.stdout);
        lines.extend(
            stdout
                .lines()
                .filter(|l| l.starts_with("[ERROR] "))
                .map(|l| l.replace(&prefix, "")),
        );
    }
    lines.sort();
    lines.dedup();
    lines
}

/// The processor reports what `checkstyle` prints for the engine's
/// fixtures that parse (a file checkstyle cannot parse stops its run with
/// a stack trace, which the engine's own tests compare).
#[test]
fn icheckstyle_matches_checkstyle() {
    crate::common::require_tool("checkstyle");
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project = temp_dir.path();
    fs::write(project.join("rsconstruct.toml"), CONFIG).expect("write rsconstruct.toml");
    fs::write(project.join("checkstyle.xml"), CHECKSTYLE_XML).expect("write checkstyle.xml");
    fs::create_dir(project.join("src")).expect("create src");
    let mut names = Vec::new();
    for (name, contents) in engine_fixtures() {
        fs::write(project.join("src").join(&name), contents).expect("write a fixture");
        if !name.starts_with("Err") {
            names.push(format!("src/{name}"));
        }
    }
    let output = run_rsconstruct_with_env(project, &["build", "-k"], &[("NO_COLOR", "1")]);
    let expected = checkstyle_messages(project, &names);
    assert!(
        expected.len() > 20,
        "the fixtures have messages: {expected:?}"
    );
    assert_eq!(
        reported_messages(&output),
        expected,
        "icheckstyle (left) and checkstyle (right)"
    );
}

const CONFIG_FILE_CONFIG: &str =
    "[processor.checker.icheckstyle]\nsrc_dirs = [\"src\"]\nconfig_file = \"conf/checks.xml\"\n";

/// A configuration with one check that `long x = 1l;` passes.
const ARRAY_TYPE_STYLE_XML: &str = "<?xml version=\"1.0\"?>\n<module name=\"Checker\">\n  <module name=\"TreeWalker\">\n    <module name=\"ArrayTypeStyle\"/>\n  </module>\n</module>\n";

/// A configuration with one check that `long x = 1l;` fails.
const UPPER_ELL_XML: &str = "<?xml version=\"1.0\"?>\n<module name=\"Checker\">\n  <module name=\"TreeWalker\">\n    <module name=\"UpperEll\"/>\n  </module>\n</module>\n";

/// A `config_file` other than `checkstyle.xml` is an input of every
/// product: editing it re-checks a file that passed, without a `dep_inputs`
/// entry naming it.
#[test]
fn icheckstyle_config_file_edit_rechecks() {
    let temp_dir = setup_project(
        CONFIG_FILE_CONFIG,
        &[
            ("conf/checks.xml", ARRAY_TYPE_STYLE_XML),
            ("src/Ell.java", "class Ell {\n  long x = 1l;\n}\n"),
        ],
    );
    assert_passes(
        &build(temp_dir.path()),
        "a lowercase ell while the named config leaves UpperEll off",
    );
    fs::write(temp_dir.path().join("conf/checks.xml"), UPPER_ELL_XML).unwrap();
    assert_fails_with(
        &build(temp_dir.path()),
        "the same file once the config enables UpperEll",
        "Should use uppercase 'L'. [UpperEll]",
    );
}

/// A `config_file` that does not exist fails the build naming it.
#[test]
fn icheckstyle_missing_config_file_fails() {
    let temp_dir = setup_project(
        CONFIG_FILE_CONFIG,
        &[("src/Ell.java", "class Ell {\n  long x = 1L;\n}\n")],
    );
    assert_fails_with(
        &build(temp_dir.path()),
        "a missing configuration",
        "icheckstyle config_file not found: conf/checks.xml",
    );
}

/// Not batched, as the docs say: files are checked in parallel as separate
/// products. A batch would run them one after another on a single worker.
#[test]
fn icheckstyle_does_not_batch() {
    let info = crate::common::processor_info("processor.checker.icheckstyle");
    assert_eq!(info["supports_batch"], false);
}
