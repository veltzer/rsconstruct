use crate::common::run_rsconstruct_with_env;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

const CONFIG: &str = "[processor.checker.iperlcritic]\nsrc_dirs = [\"src\"]\n";

/// The fleet's `.perlcriticrc`.
const PERLCRITICRC: &str = "# perlcritic config, shared fleet-wide (rsmultigit check same).
# Severity 5 is the \"must fix\" level; lower the number to tighten the net.
severity = 5
verbose = %f:%l:%c: [%p] %m\\n
";

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

/// Not batched, as the docs say: files are checked in parallel as separate
/// products. A batch would run them one after another on a single worker.
#[test]
fn iperlcritic_does_not_batch() {
    let info = crate::common::processor_info("processor.checker.iperlcritic");
    assert_eq!(info["supports_batch"], false);
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
fn iperlcritic_clean_file_passes() {
    let temp_dir = setup_project(
        CONFIG,
        &[
            (".perlcriticrc", PERLCRITICRC),
            (
                "src/ok.pl",
                "use strict;\nuse warnings;\nprint \"hello\\n\";\n",
            ),
        ],
    );
    assert_passes(&build(temp_dir.path()), "a clean Perl file");
}

#[test]
fn iperlcritic_reports_in_the_profile_format() {
    let temp_dir = setup_project(
        CONFIG,
        &[
            (".perlcriticrc", PERLCRITICRC),
            (
                "src/bad.pl",
                "print 1;\nuse strict;\nopen(FH, $f);\nsub f { return undef }\n",
            ),
        ],
    );
    let output = build(temp_dir.path());
    assert_fails_with(
        &output,
        "code before strictures",
        "src/bad.pl:1:1: [TestingAndDebugging::RequireUseStrict] Code before strictures are enabled",
    );
    assert_fails_with(
        &output,
        "a two-argument open",
        "src/bad.pl:3:1: [InputOutput::ProhibitTwoArgOpen] Two-argument \"open\" used",
    );
    assert_fails_with(
        &output,
        "an explicit return undef",
        "src/bad.pl:4:9: [Subroutines::ProhibitExplicitReturnUndef] \"return\" statement with explicit \"undef\"",
    );
}

#[test]
fn iperlcritic_reports_parse_errors() {
    let temp_dir = setup_project(
        CONFIG,
        &[
            (".perlcriticrc", PERLCRITICRC),
            ("src/broken.pl", "use strict;\nmy $x = 1; \u{e9};\n"),
        ],
    );
    assert_fails_with(
        &build(temp_dir.path()),
        "a character PPI cannot tokenize",
        "Problem while critiquing \"src/broken.pl\": Can't parse code: Encountered unexpected character '195'",
    );
}

#[test]
fn iperlcritic_honours_no_critic_annotations() {
    let temp_dir = setup_project(
        CONFIG,
        &[
            (".perlcriticrc", PERLCRITICRC),
            (
                "src/annotated.pl",
                "use strict;\nopen(FH, $f); ## no critic (TwoArgOpen BarewordFileHandles)\n",
            ),
        ],
    );
    assert_passes(&build(temp_dir.path()), "violations under ## no critic");
}

#[test]
fn iperlcritic_reads_the_profile_field() {
    let config =
        "[processor.checker.iperlcritic]\nsrc_dirs = [\"src\"]\nprofile = \"conf/critic.rc\"\n";
    let profile = "verbose = %f:%l: %m\\n\n[-Subroutines::ProhibitExplicitReturnUndef]\n";
    let temp_dir = setup_project(
        config,
        &[
            ("conf/critic.rc", profile),
            (
                "src/r.pl",
                "use strict;\nsub f { return undef }\nopen(FH, $f);\n",
            ),
        ],
    );
    let output = build(temp_dir.path());
    assert_fails_with(
        &output,
        "a two-argument open",
        "src/r.pl:3: Two-argument \"open\" used",
    );
    assert!(
        !combined_output(&output).contains("explicit \"undef\""),
        "a disabled policy reports nothing: {}",
        combined_output(&output)
    );
}

#[test]
fn iperlcritic_without_a_profile_uses_the_defaults() {
    let temp_dir = setup_project(CONFIG, &[("src/d.pl", "use strict;\nreturn sort @x;\n")]);
    assert_fails_with(
        &build(temp_dir.path()),
        "the default format, with the file name",
        "src/d.pl: \"return\" statement followed by \"sort\" at line 2, column 1.  Behavior is undefined if called in scalar context.  (Severity: 5)",
    );
}

#[test]
fn iperlcritic_rejects_unported_policies() {
    let temp_dir = setup_project(
        CONFIG,
        &[
            (".perlcriticrc", "severity = 3\n"),
            ("src/ok.pl", "use strict;\n"),
        ],
    );
    assert_fails_with(
        &build(temp_dir.path()),
        "a profile enabling lower severities",
        "iperlcritic does not implement policy",
    );
}

/// The engine's fixtures (`src/engines/perlcritic/testdata`), with their
/// paths under it.
fn engine_fixtures() -> Vec<(String, Vec<u8>)> {
    fn walk(dir: &Path, prefix: &str, out: &mut Vec<(String, Vec<u8>)>) {
        for entry in fs::read_dir(dir).expect("read the engine's testdata") {
            let path = entry.expect("testdata entry").path();
            let name = format!("{prefix}{}", path.file_name().unwrap().to_string_lossy());
            if path.is_dir() {
                walk(&path, &format!("{name}/"), out);
            } else {
                out.push((name, fs::read(&path).expect("read a fixture")));
            }
        }
    }
    let mut files = Vec::new();
    walk(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src/engines/perlcritic/testdata"),
        "",
        &mut files,
    );
    files.sort();
    files
}

/// The violation lines of a build's failed products.
fn reported_lines(output: &std::process::Output) -> Vec<String> {
    let text = String::from_utf8_lossy(&output.stderr);
    let mut lines: Vec<String> = text
        .lines()
        .map(|l| match l.split_once("[processor.checker.iperlcritic] ") {
            Some((_, rest)) => rest,
            None => l,
        })
        // The reporter names the product before the message (`src/a.pl:
        // src/a.pl:1:1: ...`); the violation line itself names the file (a
        // `#line` directive may make that another name, as perlcritic prints
        // it). A violation's own `path:line:col` always holds a colon, so a
        // bare `src/...` before the first `: ` is the product prefix.
        .map(|l| match l.split_once(": ") {
            Some((product, rest)) if product.starts_with("src/") && !product.contains(':') => rest,
            _ => l,
        })
        .filter(|l| l.starts_with("src/") && l.contains(": ["))
        .map(str::to_string)
        .collect();
    lines.sort();
    lines.dedup();
    lines
}

/// `perlcritic` over the same files, its violation lines. The fixtures a
/// policy dies on (`fatal*.pl`) are left out: they would end the run.
fn perlcritic_lines(project: &Path, files: &[String]) -> Vec<String> {
    let out = std::process::Command::new("perlcritic")
        .current_dir(project)
        .args(files.iter().filter(|f| !f.starts_with("src/fatal")))
        .output()
        .expect("run perlcritic");
    let mut lines: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| l.starts_with("src/") && l.contains(": ["))
        .map(str::to_string)
        .collect();
    lines.sort();
    lines.dedup();
    lines
}

/// The processor reports what `perlcritic` prints for the engine's
/// fixtures, with the fleet's profile.
#[test]
fn iperlcritic_matches_perlcritic() {
    crate::common::require_tool("perlcritic");
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project = temp_dir.path();
    fs::write(project.join("rsconstruct.toml"), CONFIG).expect("write rsconstruct.toml");
    fs::write(project.join(".perlcriticrc"), PERLCRITICRC).expect("write .perlcriticrc");
    let mut names = Vec::new();
    for (name, contents) in engine_fixtures() {
        let path = project.join("src").join(&name);
        fs::create_dir_all(path.parent().unwrap()).expect("create a directory");
        fs::write(&path, contents).expect("write a fixture");
        if name.ends_with(".pl") || name.ends_with(".pm") {
            names.push(format!("src/{name}"));
        }
    }
    let output = run_rsconstruct_with_env(project, &["build", "-k"], &[("NO_COLOR", "1")]);
    let expected = perlcritic_lines(project, &names);
    assert!(
        expected.len() > 100,
        "the fixtures have violations: {expected:?}"
    );
    assert_eq!(
        reported_lines(&output),
        expected,
        "iperlcritic (left) and perlcritic (right)"
    );
}

const PROFILE_CONFIG: &str =
    "[processor.checker.iperlcritic]\nsrc_dirs = [\"src\"]\nprofile = \"conf/perlcriticrc\"\n";

/// A `profile` other than `.perlcriticrc` is an input of every product:
/// editing it re-checks a file that passed, without a `dep_inputs` entry
/// naming it.
#[test]
fn iperlcritic_profile_edit_rechecks() {
    let temp_dir = setup_project(
        PROFILE_CONFIG,
        &[
            (
                "conf/perlcriticrc",
                "severity = 5\nverbose = %f:%l:%c: [%p] %m\\n\n[-TestingAndDebugging::RequireUseStrict]\n",
            ),
            ("src/loose.pl", "#!/usr/bin/perl\nprint \"hi\\n\";\n"),
        ],
    );
    assert_passes(
        &build(temp_dir.path()),
        "a file without strictures while the named profile disables RequireUseStrict",
    );
    fs::write(
        temp_dir.path().join("conf/perlcriticrc"),
        "severity = 5\nverbose = %f:%l:%c: [%p] %m\\n\n",
    )
    .unwrap();
    assert_fails_with(
        &build(temp_dir.path()),
        "the same file once the profile enables RequireUseStrict again",
        "[TestingAndDebugging::RequireUseStrict] Code before strictures are enabled",
    );
}

/// A `profile` that does not exist fails the build naming it, rather than
/// checking with Perl::Critic's defaults (only the default `.perlcriticrc`
/// may be absent).
#[test]
fn iperlcritic_missing_profile_fails() {
    let temp_dir = setup_project(
        PROFILE_CONFIG,
        &[(
            "src/ok.pl",
            "#!/usr/bin/perl\nuse strict;\nprint \"hi\\n\";\n",
        )],
    );
    assert_fails_with(
        &build(temp_dir.path()),
        "a missing profile",
        "iperlcritic profile not found: conf/perlcriticrc",
    );
}
