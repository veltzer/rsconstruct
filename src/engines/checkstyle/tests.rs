//! The port checked against checkstyle itself: the Java files in
//! `testdata/` and mutated copies of them are checked by both under several
//! configurations, and each file's result must be the same: the same
//! messages, line for line and in the same order, or the same exception at
//! the root of checkstyle's stack trace.
//!
//! checkstyle 8.36.1, the version ported, must be installed (CI installs
//! it): a missing or different `checkstyle` fails these tests, it never
//! skips them. checkstyle runs with `haltOnException = false`, so one
//! failing file does not stop the run and each file's exception is in the
//! output; everything else in the configuration is the test's.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::{Outcome, check_file, config};

/// The checkstyle release the engine ports.
const CHECKSTYLE_VERSION: &str = "8.36.1";

/// Mutated copies made of each fixture.
const MUTANTS_PER_FIXTURE: usize = 12;

fn testdata_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src/engines/checkstyle/testdata")
}

/// Every fixture: its name and contents.
fn fixtures() -> Vec<(String, Vec<u8>)> {
    let mut files: Vec<(String, Vec<u8>)> = std::fs::read_dir(testdata_dir())
        .expect("read testdata")
        .map(|e| {
            let path = e.expect("testdata entry").path();
            let name = path
                .file_name()
                .expect("name")
                .to_string_lossy()
                .into_owned();
            (name, std::fs::read(&path).expect("read fixture"))
        })
        .collect();
    files.sort();
    files
}

struct Lcg(u64);

impl Lcg {
    const fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

/// One to three of: a line deleted, duplicated or swapped, a character
/// deleted, a piece of Java (or of a broken token) inserted, an operator
/// flipped. The lexer's and parser's error paths and the checks' corner
/// cases are where a port diverges first.
fn mutate(source: &str, rng: &mut Lcg) -> String {
    const INSERTS: &[&str] = &[
        "{",
        "}",
        "(",
        ")",
        "[",
        "]",
        ";",
        ",",
        "\"",
        "'",
        "/*",
        "*/",
        "//",
        "\n",
        "\r",
        "\t",
        "<",
        ">",
        ">>",
        "@",
        "#",
        "\\",
        "\\u0041",
        "`",
        "->",
        "::",
        "...",
        "0x",
        "1L",
        "1l",
        "07",
        "1_",
        "1e",
        "\"\"\"",
        "case 1:",
        "default:",
        "break;",
        "return true;",
        "else",
        "if (x == \"a\")",
        "for (int i = 0; i < 3; i++) { i++; }",
        "switch (x) {",
        "yield",
        "record",
        "enum",
        "assert",
        "x = y;",
        "import java.lang.String;",
        "/** {@link Foo} @see Bar */",
        "// fall through\n",
        "true",
        "false",
        "!",
        "==",
        "\u{e9}",
        "\u{2028}",
        "\u{feff}",
    ];
    let mut lines: Vec<String> = source.split('\n').map(str::to_string).collect();
    for _ in 0..=rng.below(3) {
        if lines.is_empty() {
            lines.push(String::new());
        }
        let i = rng.below(lines.len());
        let chars: Vec<char> = lines[i].chars().collect();
        match rng.below(7) {
            0 => {
                lines.remove(i);
            }
            1 => {
                let copy = lines[rng.below(lines.len())].clone();
                lines.insert(i, copy);
            }
            2 if !chars.is_empty() => {
                let j = rng.below(chars.len());
                lines[i] = chars[..j].iter().chain(&chars[j + 1..]).collect();
            }
            3 => {
                let j = rng.below(chars.len() + 1);
                let insert = INSERTS[rng.below(INSERTS.len())];
                lines[i] = chars[..j].iter().collect::<String>()
                    + insert
                    + &chars[j..].iter().collect::<String>();
            }
            4 => {
                lines[i] = if lines[i].contains("==") {
                    lines[i].replacen("==", "!=", 1)
                } else {
                    lines[i].replacen(';', ";;", 1)
                };
            }
            5 => {
                let j = rng.below(lines.len());
                lines.swap(i, j);
            }
            _ => {
                let insert = INSERTS[rng.below(INSERTS.len())];
                lines.insert(i, insert.to_string());
            }
        }
    }
    lines.join("\n")
}

/// The fixtures and their mutants, written to a fresh directory.
struct Corpus {
    dir: tempfile::TempDir,
    files: Vec<PathBuf>,
}

fn corpus() -> Corpus {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut rng = Lcg(0x00C0_FFEE);
    let mut files = Vec::new();
    for (name, contents) in fixtures() {
        let mut write = |file_name: String, bytes: &[u8]| {
            let path = dir.path().join(file_name);
            std::fs::write(&path, bytes).expect("write");
            files.push(path);
        };
        write(name.clone(), &contents);
        let Ok(source) = String::from_utf8(contents) else {
            continue;
        };
        for i in 0..MUTANTS_PER_FIXTURE {
            write(format!("M{i}{name}"), mutate(&source, &mut rng).as_bytes());
        }
    }
    files.sort();
    Corpus { dir, files }
}

/// A file's result: its message lines, or the exception it stops on.
#[derive(Debug, PartialEq, Eq)]
enum Result {
    Messages(Vec<String>),
    Failed(String),
}

/// `checkstyle -c CONFIG FILES` with `haltOnException = false`.
fn run_checkstyle(config_xml: &str, corpus: &Corpus) -> BTreeMap<String, Result> {
    let version = Command::new("checkstyle").arg("--version").output().expect(
        "checkstyle is not installed: install it; missing tools fail tests, they never skip",
    );
    let version = String::from_utf8_lossy(&version.stdout);
    assert!(
        version.trim() == format!("Checkstyle version: {CHECKSTYLE_VERSION}"),
        "the engine ports checkstyle {CHECKSTYLE_VERSION}, the installed one is: {version}"
    );
    let config_path = corpus.dir.path().join("reference.xml");
    let reference = config_xml.replacen(
        "<module name=\"Checker\">",
        "<module name=\"Checker\">\n  <property name=\"haltOnException\" value=\"false\"/>",
        1,
    );
    std::fs::write(&config_path, reference).expect("write the configuration");
    let out = Command::new("checkstyle")
        .arg("-c")
        .arg(&config_path)
        .args(&corpus.files)
        .output()
        .expect("run checkstyle");
    parse_checkstyle_output(&String::from_utf8_lossy(&out.stdout))
}

/// checkstyle's report, by file. A file it could not check has one
/// message, "Got an exception - " and the stack trace, whose last `Caused
/// by:` (or first line) is the root exception.
fn parse_checkstyle_output(text: &str) -> BTreeMap<String, Result> {
    let mut records: Vec<Vec<&str>> = Vec::new();
    for line in text.lines() {
        if ["[ERROR] ", "[WARN] ", "[INFO] "]
            .iter()
            .any(|p| line.starts_with(p))
        {
            records.push(vec![line]);
        } else if line.starts_with("Starting audit") || line.starts_with("Audit done") {
            records.push(Vec::new());
        } else if let Some(record) = records.last_mut().filter(|r| !r.is_empty()) {
            record.push(line);
        }
    }
    let mut results: BTreeMap<String, Result> = BTreeMap::new();
    for record in records.into_iter().filter(|r| !r.is_empty()) {
        let first = record[0];
        let file_start = first.find("] ").expect("a tag") + 2;
        let file_end = first[file_start..].find(".java:").expect("a .java file") + file_start + 5;
        let file = first[file_start..file_end].to_string();
        if let Some((_, exception)) = first.split_once(" Got an exception - ") {
            // The root exception's message runs from its line to the first
            // stack frame; it spans lines when it quotes a token that does.
            let start = record.iter().rposition(|l| l.starts_with("Caused by: "));
            let (head, rest) = match start {
                Some(i) => (&record[i]["Caused by: ".len()..], &record[i + 1..]),
                None => (exception, &record[1..]),
            };
            let mut root = head.to_string();
            for line in rest
                .iter()
                .take_while(|l| !l.starts_with("\tat ") && !l.starts_with("\t..."))
            {
                root.push('\n');
                root.push_str(line);
            }
            let root = if root.starts_with(&file) {
                root
            } else {
                format!("{file}: {root}")
            };
            results.insert(file, Result::Failed(root));
            continue;
        }
        match results
            .entry(file)
            .or_insert_with(|| Result::Messages(Vec::new()))
        {
            Result::Messages(lines) => lines.push(record.join("\n")),
            Result::Failed(_) => {}
        }
    }
    results
}

/// The engine over the same files.
fn run_engine(config_xml: &str, corpus: &Corpus) -> BTreeMap<String, Result> {
    let config = config::parse(config_xml).expect("the configuration");
    let mut results = BTreeMap::new();
    for path in &corpus.files {
        let name = path.to_string_lossy().into_owned();
        let bytes = std::fs::read(path).expect("read");
        match check_file(&name, &bytes, &config) {
            Outcome::Skipped => {}
            Outcome::Failed(failure) => {
                results.insert(name.clone(), Result::Failed(failure.render(&name)));
            }
            Outcome::Checked(messages) => {
                let lines: Vec<String> = messages
                    .iter()
                    .filter(|m| m.severity != super::message::Severity::Ignore)
                    .map(|m| m.render(&name))
                    .collect();
                if !lines.is_empty() {
                    results.insert(name.clone(), Result::Messages(lines));
                }
            }
        }
    }
    results
}

fn assert_same(config_xml: &str) {
    let corpus = corpus();
    let expected = run_checkstyle(config_xml, &corpus);
    let actual = run_engine(config_xml, &corpus);
    let mut differences = Vec::new();
    let only_actual = actual.keys().filter(|f| !expected.contains_key(*f));
    for file in expected.keys().chain(only_actual) {
        let (e, a) = (expected.get(file), actual.get(file));
        if e != a {
            differences.push(format!("{file}\n  checkstyle: {e:?}\n  engine:     {a:?}"));
        }
    }
    assert!(
        differences.is_empty(),
        "{} of {} files differ:\n{}",
        differences.len(),
        corpus.files.len(),
        differences.join("\n")
    );
    assert!(
        expected.len() > corpus.files.len() / 2,
        "most files have results: {}",
        expected.len()
    );
}

fn checker(checker_properties: &str, body: &str) -> String {
    format!(
        "<?xml version=\"1.0\"?>\n<!DOCTYPE module PUBLIC \"-//Checkstyle//DTD Checkstyle Configuration 1.3//EN\" \
         \"https://checkstyle.org/dtds/configuration_1_3.dtd\">\n<module name=\"Checker\">\n{checker_properties}\n{body}\n</module>\n"
    )
}

/// Every implemented check with its default properties: the fleet's
/// `checkstyle.xml`.
const ALL_CHECKS: &str = r#"<module name="NewlineAtEndOfFile"/>
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
</module>"#;

#[test]
fn default_properties_match_checkstyle() {
    assert_same(&checker("", ALL_CHECKS));
}

#[test]
fn non_default_properties_match_checkstyle() {
    let body = r#"<module name="NewlineAtEndOfFile"><property name="lineSeparator" value="lf"/></module>
<module name="TreeWalker">
  <module name="ArrayTypeStyle"><property name="javaStyle" value="false"/></module>
  <module name="DefaultComesLast"><property name="skipIfLastAndSharedWithCase" value="true"/></module>
  <module name="FallThrough"><property name="checkLastCaseGroup" value="true"/><property name="reliefPattern" value="continue in next case"/></module>
  <module name="ModifiedControlVariable"><property name="skipEnhancedForLoopVariable" value="true"/></module>
  <module name="OneStatementPerLine"><property name="treatTryResourcesAsStatement" value="true"/></module>
  <module name="UnusedImports"><property name="processJavadoc" value="false"/><property name="id" value="unused"/></module>
  <module name="UpperEll"><message key="upperEll" value="Use L &apos;&apos;{0}&apos;&apos; please - {1}"/></module>
  <module name="EmptyStatement"><property name="severity" value="ignore"/></module>
  <module name="RedundantImport"><message key="import.duplicate" value="dup at {0}: {1}"/></module>
  <module name="CovariantEquals"/><module name="EqualsHashCode"/><module name="SimplifyBooleanExpression"/>
  <module name="SimplifyBooleanReturn"/><module name="StringLiteralEquality"/>
</module>"#;
    assert_same(&checker(r#"<property name="tabWidth" value="4"/>"#, body));
}

#[test]
fn severities_ids_and_tab_widths_match_checkstyle() {
    let body = r#"<module name="NewlineAtEndOfFile"><property name="lineSeparator" value="crlf"/><property name="severity" value="info"/></module>
<module name="TreeWalker"><property name="tabWidth" value="3"/><property name="severity" value="warning"/>
  <module name="FallThrough"/><module name="UnusedImports"><property name="severity" value="error"/></module>
  <module name="OneStatementPerLine"/><module name="ModifiedControlVariable"/><module name="DefaultComesLast"/>
  <module name="ArrayTypeStyle"/><module name="UpperEll"><property name="id" value="ell"/></module>
  <module name="StringLiteralEquality"><property name="id" value="a-id"/></module>
  <module name="SimplifyBooleanExpression"><property name="id" value="a-id"/></module>
  <module name="EmptyStatement"><property name="tabWidth" value="2"/></module>
</module>"#;
    assert_same(&checker(r#"<property name="tabWidth" value="1"/>"#, body));
}

/// The fixtures cover what they are for: every check reports somewhere,
/// and every kind of failure happens.
#[test]
fn fixtures_exercise_every_check_and_failure() {
    let corpus = Corpus {
        files: fixtures()
            .iter()
            .map(|(name, _)| testdata_dir().join(name))
            .collect(),
        dir: tempfile::tempdir().expect("tempdir"),
    };
    let results = run_engine(&checker("", ALL_CHECKS), &corpus);
    let all = format!("{results:?}");
    for check in [
        "NewlineAtEndOfFile",
        "ArrayTypeStyle",
        "CovariantEquals",
        "DefaultComesLast",
        "EmptyStatement",
        "EqualsHashCode",
        "FallThrough",
        "ModifiedControlVariable",
        "OneStatementPerLine",
        "RedundantImport",
        "SimplifyBooleanExpression",
        "SimplifyBooleanReturn",
        "StringLiteralEquality",
        "UnusedImports",
        "UpperEll",
    ] {
        assert!(
            all.contains(&format!("[{check}]")),
            "no fixture triggers {check}"
        );
    }
    for failure in [
        "unexpected token",
        "expecting",
        "Unexpected character",
        "unexpected char",
        "StringIndexOutOfBoundsException",
        "areLtsAndGtsBalanced",
    ] {
        assert!(all.contains(failure), "no fixture fails with {failure:?}");
    }
}
