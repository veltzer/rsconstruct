//! The port checked against `shellcheck` itself: the scripts in `testdata/`
//! and mutated copies of them are checked by both, under each option the
//! port supports, and the findings (`shellcheck --format=gcc`) must be the
//! same, line for line and in the same order.
//!
//! `shellcheck` 0.11.0, the version ported, must be installed (CI installs
//! it): a missing or different `shellcheck` fails these tests, it never
//! skips them. Both sides run with `--norc` unless the test names an
//! `--rcfile`, so no `.shellcheckrc` around the machine changes the result,
//! and both resolve relative `source` paths against this process's working
//! directory.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::data::shell_for_executable;
use super::interface::Severity;
use super::{CheckSpec, IoOptions, check_files};

/// The `shellcheck` release the engine ports.
const SHELLCHECK_VERSION: &str = "0.11.0";

/// The parser recurses once per nesting level, and the tests run unoptimized,
/// so checks run on a thread with a large stack, as the processor's do.
const CHECK_STACK_SIZE: usize = 256 * 1024 * 1024;

/// Mutated copies made of each fixture.
const MUTANTS_PER_FIXTURE: usize = 8;

/// The fixtures, copied to a fresh directory.
struct Fixtures {
    dir: tempfile::TempDir,
    files: Vec<String>,
}

fn testdata_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src/engines/shellcheck/testdata")
}

/// Every fixture, by name.
fn fixture_names() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(testdata_dir())
        .expect("read testdata")
        .map(|e| {
            e.expect("testdata entry")
                .file_name()
                .into_string()
                .expect("name")
        })
        .collect();
    names.sort();
    names
}

fn stage(files: &[(String, Vec<u8>)]) -> Fixtures {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut paths = Vec::new();
    for (name, contents) in files {
        let path = dir.path().join(name);
        std::fs::write(&path, contents).expect("write fixture");
        paths.push(path.to_string_lossy().into_owned());
    }
    Fixtures { dir, files: paths }
}

fn fixtures() -> Fixtures {
    let files: Vec<(String, Vec<u8>)> = fixture_names()
        .into_iter()
        .map(|name| {
            let contents = std::fs::read(testdata_dir().join(&name)).expect("read fixture");
            (name, contents)
        })
        .collect();
    stage(&files)
}

/// The engine's options for a `shellcheck` command line.
fn engine_options(args: &[String]) -> (CheckSpec, IoOptions) {
    let mut spec = CheckSpec::new("", String::new());
    let mut io = IoOptions::default();
    let code = |s: &str| -> i64 { s.trim_start_matches("SC").parse().expect("a code") };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = || it.next().expect("an option value").as_str();
        match arg.as_str() {
            "--norc" => spec.ignore_rc = true,
            "-x" => io.external_sources = true,
            "-a" => spec.check_sourced = true,
            "-s" => spec.shell_type_override = Some(shell_for_executable(value()).expect("shell")),
            "-o" => spec.optional_checks.push(value().to_string()),
            "-e" => spec.excluded_warnings.extend(value().split(',').map(code)),
            "-i" => spec
                .included_warnings
                .get_or_insert_with(Vec::new)
                .extend(value().split(',').map(code)),
            "-S" => {
                spec.min_severity = match value() {
                    "error" => Severity::ErrorC,
                    "warning" => Severity::WarningC,
                    "info" => Severity::InfoC,
                    "style" => Severity::StyleC,
                    other => panic!("severity {other}"),
                }
            }
            "-P" => io.source_paths.push(value().to_string()),
            "--rcfile" => io.rcfile = Some(value().to_string()),
            "--extended-analysis" => {
                spec.extended_analysis = Some(value().parse().expect("true or false"));
            }
            other => panic!("no engine option for {other}"),
        }
    }
    (spec, io)
}

/// `shellcheck --format=gcc ARGS FILES`, its output lines.
fn run_shellcheck(args: &[String], files: &[String]) -> Vec<String> {
    let version = Command::new("shellcheck").arg("--version").output().expect(
        "shellcheck is not installed: install it; missing tools fail tests, they never skip",
    );
    let version = String::from_utf8_lossy(&version.stdout);
    assert!(
        version
            .lines()
            .any(|l| l == format!("version: {SHELLCHECK_VERSION}")),
        "the engine ports shellcheck {SHELLCHECK_VERSION}, the installed one is:\n{version}"
    );
    let out = Command::new("shellcheck")
        .arg("--format=gcc")
        .args(args)
        .args(files)
        .output()
        .expect("run shellcheck");
    assert!(
        out.stderr.is_empty(),
        "shellcheck {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::to_string)
        .collect()
}

/// The engine over the same files, its output lines.
fn run_engine(args: &[String], files: &[String]) -> Vec<String> {
    let (spec, io) = engine_options(args);
    let files = files.to_vec();
    std::thread::Builder::new()
        .stack_size(CHECK_STACK_SIZE)
        .spawn(move || check_files(&files, io, &spec))
        .expect("spawn the check thread")
        .join()
        .expect("the engine panicked")
}

/// Checks the files with both and requires the same findings, reporting
/// the first file whose findings differ.
fn assert_same(args: &[&str], files: &[String]) {
    let args: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
    let expected = run_shellcheck(&args, files);
    let actual = run_engine(&args, files);
    if expected == actual {
        return;
    }
    for file in files {
        let of_file = |lines: &[String]| -> Vec<String> {
            lines
                .iter()
                .filter(|l| l.starts_with(&format!("{file}:")))
                .cloned()
                .collect()
        };
        let (e, a) = (of_file(&expected), of_file(&actual));
        assert_eq!(
            e,
            a,
            "{file} with {args:?}: shellcheck (left) and the engine (right) differ; the file:\n{}",
            String::from_utf8_lossy(&std::fs::read(file).unwrap_or_default())
        );
    }
    assert_eq!(expected, actual, "same findings per file, different order");
}

fn assert_same_on_fixtures(args: &[&str]) {
    let f = fixtures();
    assert_same(args, &f.files);
    drop(f.dir);
}

#[test]
fn same_as_shellcheck_with_defaults() {
    assert_same_on_fixtures(&["--norc"]);
}

#[test]
fn same_as_shellcheck_for_each_shell() {
    for shell in ["sh", "bash", "dash", "ksh", "busybox"] {
        assert_same_on_fixtures(&["--norc", "-s", shell]);
    }
}

#[test]
fn same_as_shellcheck_with_optional_checks() {
    assert_same_on_fixtures(&["--norc", "-o", "all"]);
    assert_same_on_fixtures(&[
        "--norc",
        "-o",
        "require-variable-braces",
        "-o",
        "check-unassigned-uppercase",
    ]);
}

#[test]
fn same_as_shellcheck_following_sources() {
    assert_same_on_fixtures(&["--norc", "-x"]);
    assert_same_on_fixtures(&["--norc", "-x", "-a"]);
    assert_same_on_fixtures(&["--norc", "-a"]);
    assert_same_on_fixtures(&["--norc", "-x", "-P", "SCRIPTDIR"]);
}

#[test]
fn same_as_shellcheck_filtering_findings() {
    assert_same_on_fixtures(&["--norc", "-S", "error"]);
    assert_same_on_fixtures(&["--norc", "-S", "warning"]);
    assert_same_on_fixtures(&["--norc", "-S", "info"]);
    assert_same_on_fixtures(&["--norc", "-e", "SC2086,SC2034", "-e", "1091"]);
    assert_same_on_fixtures(&["--norc", "-i", "SC2086,SC2154"]);
    assert_same_on_fixtures(&["--norc", "--extended-analysis", "false"]);
}

#[test]
fn same_as_shellcheck_with_an_rcfile() {
    let f = fixtures();
    let rc = f.dir.path().join("checkrc");
    std::fs::write(
        &rc,
        "disable=SC2086\nenable=require-variable-braces\nshell=bash\nexternal-sources=true\nsource-path=SCRIPTDIR\n",
    )
    .expect("write the rcfile");
    let rc = rc.to_string_lossy().into_owned();
    assert_same(&["--rcfile", &rc], &f.files);
}

/// A small deterministic generator (an LCG), so the mutants are the same
/// on every run.
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

/// One to three of: a line deleted, duplicated or swapped with the next,
/// a character deleted or inserted, a keyword or quote replaced. The
/// parser's error paths, which the fixtures reach only once each, are
/// where a port diverges first.
fn mutate(source: &str, rng: &mut Lcg) -> String {
    const INSERTS: &[char] = &[
        '"', '\'', '(', ')', '{', '}', '[', ']', '$', ';', '&', '|', '<', '>', '`', '#', '=', '\n',
        '\\', ' ',
    ];
    const SWAPS: &[(&str, &str)] = &[
        ("then", ""),
        ("fi", "done"),
        ("do", "then"),
        ("esac", "fi"),
        ("done", ""),
        ("\"", "'"),
        ("[[", "["),
        ("]]", "]"),
        ("$(", "`"),
        ("&&", "||"),
        (";;", ";"),
        ("<<", "<"),
    ];
    let mut text = source.to_string();
    for _ in 0..=rng.below(3) {
        let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
        let chars: Vec<char> = text.chars().collect();
        text = match rng.below(6) {
            0 if !lines.is_empty() => {
                lines.remove(rng.below(lines.len()));
                lines.join("\n") + "\n"
            }
            1 if !lines.is_empty() => {
                let i = rng.below(lines.len());
                lines.insert(i, lines[i].clone());
                lines.join("\n") + "\n"
            }
            2 if lines.len() > 1 => {
                let i = rng.below(lines.len() - 1);
                lines.swap(i, i + 1);
                lines.join("\n") + "\n"
            }
            3 if !chars.is_empty() => {
                let i = rng.below(chars.len());
                chars[..i].iter().chain(&chars[i + 1..]).collect()
            }
            4 => {
                let i = rng.below(chars.len() + 1);
                let c = INSERTS[rng.below(INSERTS.len())];
                chars[..i]
                    .iter()
                    .chain(std::iter::once(&c))
                    .chain(&chars[i..])
                    .collect()
            }
            _ => {
                let (from, to) = SWAPS[rng.below(SWAPS.len())];
                let hits: Vec<usize> = text.match_indices(from).map(|(i, _)| i).collect();
                if hits.is_empty() {
                    text
                } else {
                    let at = hits[rng.below(hits.len())];
                    format!("{}{to}{}", &text[..at], &text[at + from.len()..])
                }
            }
        };
    }
    text
}

fn mutants() -> Fixtures {
    let mut rng = Lcg(0x5EED);
    let mut files = Vec::new();
    for name in fixture_names() {
        let bytes = std::fs::read(testdata_dir().join(&name)).expect("read fixture");
        // Mutated as text; the fixtures that are not UTF-8 test the decoder
        // as they are.
        let Ok(source) = String::from_utf8(bytes) else {
            continue;
        };
        for i in 0..MUTANTS_PER_FIXTURE {
            files.push((
                format!("m{i}_{name}"),
                mutate(&source, &mut rng).into_bytes(),
            ));
        }
    }
    stage(&files)
}

#[test]
fn same_as_shellcheck_on_mutants() {
    let m = mutants();
    assert_same(&["--norc"], &m.files);
    assert_same(&["--norc", "-s", "sh", "-o", "all"], &m.files);
    drop(m.dir);
}

/// Every fixture is one `shellcheck` finds something in, except the one
/// meant to be clean, so a fixture that stops exercising anything shows.
#[test]
fn fixtures_have_findings() {
    let f = fixtures();
    let lines = run_shellcheck(&["--norc".to_string()], &f.files);
    for file in &f.files {
        let clean = file.ends_with("/no_newline.sh");
        let found = lines.iter().any(|l| l.starts_with(&format!("{file}:")));
        assert_eq!(found, !clean, "{file}: findings expected: {}", !clean);
    }
}
