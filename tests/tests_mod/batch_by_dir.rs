//! `batch_by_dir`: a batching processor runs its tool once per directory, on
//! exactly that directory's files.

use crate::common::{make_executable, run_rsconstruct_json_with_env, run_rsconstruct_with_env};
use std::fs;
use std::path::Path;
use tempfile::TempDir;

/// A project with a fake cppcheck (`toolbin/fakecheck`) that appends one
/// line per invocation, its file arguments, to `calls.log`, and fails when
/// one of its files contains `BAD`. Directories `a` (main.c, util.c) and
/// `b` (main.c, helper.c) hold the sources.
fn project(extra_stanza: &str, build_section: &str) -> TempDir {
    let temp_dir = TempDir::new().unwrap();
    let p = temp_dir.path();
    fs::create_dir_all(p.join("toolbin")).unwrap();
    let tool = p.join("toolbin/fakecheck");
    fs::write(
        &tool,
        "#!/bin/sh\n\
         echo \"$*\" >> calls.log\n\
         for f in \"$@\"; do grep -q BAD \"$f\" && { echo \"bad file $f\" >&2; exit 1; }; done\n\
         exit 0\n",
    )
    .unwrap();
    make_executable(&tool);
    fs::write(
        p.join("rsconstruct.toml"),
        format!(
            "[build]\nhash_tool_versions = false\nexecution_model = \"native\"\n{build_section}\n\
             [processor.checker.cppcheck]\ncommand = \"fakecheck\"\nargs = []\n\
             dep_auto = []\nsrc_dirs = [\"src\"]\nbatch_by_dir = true\n{extra_stanza}"
        ),
    )
    .unwrap();
    for (dir, files) in [("a", ["main.c", "util.c"]), ("b", ["main.c", "helper.c"])] {
        fs::create_dir_all(p.join("src").join(dir)).unwrap();
        for f in files {
            fs::write(p.join("src").join(dir).join(f), format!("// {dir}/{f}\n")).unwrap();
        }
    }
    temp_dir
}

fn build(p: &Path, extra: &[&str]) -> crate::common::BuildResult {
    let path_env = format!(
        "{}:{}",
        p.join("toolbin").display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut args = vec!["build"];
    args.extend_from_slice(extra);
    run_rsconstruct_json_with_env(p, &args, &[("NO_COLOR", "1"), ("PATH", &path_env)])
}

/// The invocations recorded so far, each as its sorted file list.
fn calls(p: &Path) -> Vec<Vec<String>> {
    let mut calls: Vec<Vec<String>> = fs::read_to_string(p.join("calls.log"))
        .unwrap_or_default()
        .lines()
        .map(|l| {
            let mut files: Vec<String> = l.split_whitespace().map(String::from).collect();
            files.sort();
            files
        })
        .collect();
    calls.sort();
    calls
}

fn files(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_string()).collect()
}

/// Two directories give two invocations, each holding only its own files,
/// even with `batch_size = 1`, which would split a plain batch per file.
#[test]
fn one_invocation_per_directory() {
    let temp_dir = project("", "batch_size = 1\n");
    let p = temp_dir.path();

    let result = build(p, &["-j", "4"]);
    assert!(result.exit_success, "{:?}", result.errors);
    assert_eq!(result.success, 4, "{result:?}");
    assert_eq!(
        calls(p),
        vec![
            files(&["src/a/main.c", "src/a/util.c"]),
            files(&["src/b/helper.c", "src/b/main.c"]),
        ]
    );
}

/// Changing one file rechecks its whole directory in one invocation, and
/// leaves the other directory alone.
#[test]
fn a_changed_file_rechecks_its_whole_directory() {
    let temp_dir = project("", "");
    let p = temp_dir.path();
    assert!(build(p, &[]).exit_success);
    fs::remove_file(p.join("calls.log")).unwrap();

    let again = build(p, &[]);
    assert!(again.exit_success);
    assert_eq!(again.skipped, 4, "{again:?}");
    assert!(calls(p).is_empty(), "an unchanged build runs nothing");

    fs::write(p.join("src/a/util.c"), "// changed\n").unwrap();
    let result = build(p, &[]);
    assert!(result.exit_success, "{:?}", result.errors);
    assert_eq!(calls(p), vec![files(&["src/a/main.c", "src/a/util.c"])]);
}

/// A failing directory fails every product of that directory and no other.
#[test]
fn a_failure_is_attributed_to_its_directory() {
    let temp_dir = project("", "");
    let p = temp_dir.path();
    fs::write(p.join("src/b/helper.c"), "BAD\n").unwrap();

    let result = build(p, &["--keep-going"]);
    assert!(!result.exit_success);
    assert_eq!(result.failed, 2, "{result:?}");
    assert_eq!(result.success, 2, "{result:?}");
}

/// A processor that cannot batch rejects the field.
#[test]
fn a_processor_that_cannot_batch_rejects_it() {
    let temp_dir = TempDir::new().unwrap();
    let p = temp_dir.path();
    fs::write(
        p.join("rsconstruct.toml"),
        "[processor.checker.make]\nbatch_by_dir = true\n",
    )
    .unwrap();
    let output = run_rsconstruct_with_env(p, &["build"], &[("NO_COLOR", "1")]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unknown field 'batch_by_dir'"),
        "unexpected stderr: {stderr}"
    );
}

/// An explicit `batch = false` contradicts `batch_by_dir = true`.
#[test]
fn batch_false_contradicts_batch_by_dir() {
    let temp_dir = TempDir::new().unwrap();
    let p = temp_dir.path();
    fs::write(
        p.join("rsconstruct.toml"),
        "[processor.checker.ruff]\nbatch = false\nbatch_by_dir = true\n",
    )
    .unwrap();
    let output = run_rsconstruct_with_env(p, &["build"], &[("NO_COLOR", "1")]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("contradicts 'batch = false'"), "{stderr}");
}

/// The field is listed for a batching processor and absent for one that
/// cannot batch.
#[test]
fn defconfig_shows_the_field_only_where_it_applies() {
    let shown = crate::common::processor_defconfig("processor.checker.cppcheck");
    assert_eq!(shown["batch_by_dir"], serde_json::json!(false));
    let hidden = crate::common::processor_defconfig("processor.checker.make");
    assert!(hidden.get("batch_by_dir").is_none());
}
