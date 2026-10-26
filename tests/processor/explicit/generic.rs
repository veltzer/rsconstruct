//! The explicit processor under fixed-point discovery: one stanza is one
//! product, however many rounds it takes for its inputs to appear. The
//! upstream generator is tera (`tera.templates/gen.txt.tera` renders to
//! `gen.txt` at the project root), so `gen.txt` is absent from disk and from
//! the file index on round 0 and only shows up as a declared output on
//! round 1. The command records its argv so the tests can see what the
//! explicit product was actually run with.

use crate::common::{
    make_executable, run_rsconstruct_json_with_env, run_rsconstruct_with_env, write_file,
};
use std::fs;
use std::path::Path;
use tempfile::TempDir;

const RECORD_SCRIPT: &str = r#"#!/bin/bash
set -e
mkdir -p outdir
echo "$@" > outdir/result.txt
echo run >> runs.log
"#;

fn setup(explicit_stanza: &str) -> TempDir {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let root = temp_dir.path();
    write_file(root, "gen.sh", RECORD_SCRIPT);
    make_executable(&root.join("gen.sh"));
    write_file(root, "tera.templates/gen.txt.tera", "generated\n");
    write_file(root, "a.txt", "aaa\n");
    write_file(
        root,
        "rsconstruct.toml",
        &format!(
            "[processor.generator.tera]\nsrc_dirs = [\"tera.templates\"]\n\n{explicit_stanza}"
        ),
    );
    temp_dir
}

fn argv_recorded(root: &Path) -> String {
    fs::read_to_string(root.join("outdir/result.txt")).expect("the explicit command ran")
}

fn runs(root: &Path) -> usize {
    fs::read_to_string(root.join("runs.log")).map_or(0, |s| s.lines().count())
}

/// A literal `inputs` entry that only an upstream generator produces, with
/// `output_dirs` only. Round 0 resolved nothing and used to add a product
/// with no inputs; round 1 added a second one; the build then panicked in
/// `primary_input` ("internal error: product has no inputs").
#[test]
fn generic_upstream_generated_literal_input_with_output_dirs_only() {
    let temp_dir = setup(
        r#"[processor.explicit.generic.collect]
command = "./gen.sh"
inputs = ["gen.txt"]
output_dirs = ["outdir"]
"#,
    );
    let root = temp_dir.path();

    let result = run_rsconstruct_json_with_env(root, &["build"], &[("NO_COLOR", "1")]);
    assert!(result.exit_success, "build failed: {result:?}");
    assert_eq!(
        result.total_products, 2,
        "one tera product and one explicit product: {result:?}"
    );
    assert_eq!(result.count_status("success"), 2, "{result:?}");
    assert_eq!(runs(root), 1, "the explicit command ran once");
    assert_eq!(
        argv_recorded(root).trim(),
        "--inputs gen.txt --output-dirs outdir"
    );

    let again = run_rsconstruct_json_with_env(root, &["build"], &[("NO_COLOR", "1")]);
    assert!(again.exit_success, "{again:?}");
    assert_eq!(
        again.success, 0,
        "nothing changed, nothing rebuilt: {again:?}"
    );
    assert_eq!(runs(root), 1);
}

/// With a generated file first in `inputs` and a real file second, round 0
/// keyed the checker-style dedup on `a.txt` and round 1 on `gen.txt`, so
/// the stanza became two products ("3 built"). The product's identity is
/// its instance, and its input order is the declared order.
#[test]
fn generic_mixed_literal_inputs_with_output_dirs_only_is_one_product() {
    let temp_dir = setup(
        r#"[processor.explicit.generic.collect]
command = "./gen.sh"
inputs = ["gen.txt", "a.txt"]
output_dirs = ["outdir"]
"#,
    );
    let root = temp_dir.path();

    let result = run_rsconstruct_json_with_env(root, &["build"], &[("NO_COLOR", "1")]);
    assert!(result.exit_success, "build failed: {result:?}");
    assert_eq!(result.total_products, 2, "{result:?}");
    assert_eq!(result.count_status("success"), 2, "{result:?}");
    assert_eq!(runs(root), 1, "the explicit command ran once");
    assert_eq!(
        argv_recorded(root).trim(),
        "--inputs gen.txt a.txt --output-dirs outdir",
        "inputs are passed in declared order"
    );

    // An edit to the generated file's source reaches the explicit product.
    write_file(root, "tera.templates/gen.txt.tera", "regenerated\n");
    let again = run_rsconstruct_json_with_env(root, &["build"], &[("NO_COLOR", "1")]);
    assert!(again.exit_success, "{again:?}");
    assert_eq!(again.success, 2, "both products rebuild: {again:?}");
    assert_eq!(runs(root), 2);
}

/// `input_globs` that match only upstream outputs resolve to nothing on
/// round 0: the stanza waits for the round that injects them, and is still
/// a single product.
#[test]
fn generic_input_globs_matching_only_upstream_outputs_wait_a_round() {
    let temp_dir = setup(
        r#"[processor.explicit.generic.collect]
command = "./gen.sh"
input_globs = ["gen*.txt"]
output_dirs = ["outdir"]
"#,
    );
    let root = temp_dir.path();

    let result = run_rsconstruct_json_with_env(root, &["build"], &[("NO_COLOR", "1")]);
    assert!(result.exit_success, "build failed: {result:?}");
    assert_eq!(result.total_products, 2, "{result:?}");
    assert_eq!(runs(root), 1);
    assert_eq!(
        argv_recorded(root).trim(),
        "--inputs gen.txt --output-dirs outdir"
    );
}

/// A literal `inputs` entry that exists neither on disk nor as any
/// processor's declared output is a config error naming the file — it used
/// to be dropped without a word, and the command ran without it.
#[test]
fn generic_missing_literal_input_is_a_config_error() {
    let temp_dir = setup(
        r#"[processor.explicit.generic.collect]
command = "./gen.sh"
inputs = ["gen.txt", "a.txt", "nope.txt"]
output_dirs = ["outdir"]
"#,
    );
    let root = temp_dir.path();

    let output = run_rsconstruct_with_env(root, &["build"], &[("NO_COLOR", "1")]);
    let text = format!(
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        output.status.code(),
        Some(2),
        "config error exit code: {text}"
    );
    assert!(
        text.contains(
            "[processor.explicit.generic.collect] inputs entry 'nope.txt' does not exist"
        ),
        "the error names the stanza and the file: {text}"
    );
    assert!(
        !text.contains("gen.txt' does not exist"),
        "the upstream-generated input is not reported: {text}"
    );
    assert_eq!(runs(root), 0, "nothing ran");

    // `clean` is tolerant of it: the outputs of a build whose sources have
    // since gone must still be removable.
    let clean = run_rsconstruct_with_env(root, &["clean", "outputs"], &[("NO_COLOR", "1")]);
    assert!(
        clean.status.success(),
        "clean must not fail on a missing input: {}",
        String::from_utf8_lossy(&clean.stderr)
    );
}

/// The `inputs` and `input_globs` lists are part of the cache key: they
/// are passed to the command as `--inputs`, so listing the same files
/// under another spelling is a different run.
#[test]
fn generic_input_list_change_is_a_config_change() {
    let temp_dir = setup(
        r#"[processor.explicit.generic.collect]
command = "./gen.sh"
inputs = ["gen.txt", "a.txt"]
output_dirs = ["outdir"]
"#,
    );
    let root = temp_dir.path();
    let first = run_rsconstruct_json_with_env(root, &["build"], &[("NO_COLOR", "1")]);
    assert!(first.exit_success, "{first:?}");
    assert_eq!(runs(root), 1);

    // Same files, other order: the command sees a different argv.
    write_file(
        root,
        "rsconstruct.toml",
        r#"[processor.generator.tera]
src_dirs = ["tera.templates"]

[processor.explicit.generic.collect]
command = "./gen.sh"
inputs = ["a.txt", "gen.txt"]
output_dirs = ["outdir"]
"#,
    );
    let second = run_rsconstruct_json_with_env(root, &["build"], &[("NO_COLOR", "1")]);
    assert!(second.exit_success, "{second:?}");
    assert_eq!(runs(root), 2, "a reordered input list reruns the command");
    assert_eq!(
        argv_recorded(root).trim(),
        "--inputs a.txt gen.txt --output-dirs outdir"
    );
}
