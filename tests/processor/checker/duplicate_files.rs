//! Integration tests for the duplicate_files checker.

use crate::common::run_rsconstruct_with_env;
use std::fs;
use tempfile::TempDir;

fn setup_project(files: &[(&str, &str)]) -> TempDir {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let p = temp_dir.path();
    fs::write(
        p.join("rsconstruct.toml"),
        "[processor.checker.duplicate_files]\nsrc_dirs = [\".\"]\n",
    )
    .unwrap();
    for (name, content) in files {
        fs::write(p.join(name), content).unwrap();
    }
    temp_dir
}

#[test]
fn duplicate_files_detects_duplicates() {
    let temp_dir = setup_project(&[
        ("a.md", "same content\n"),
        ("b.md", "same content\n"),
        ("c.md", "different content\n"),
    ]);
    let output = run_rsconstruct_with_env(temp_dir.path(), &["build"], &[("NO_COLOR", "1")]);
    assert!(
        !output.status.success(),
        "build should fail with duplicate files"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("duplicate"),
        "should mention duplicates: {}",
        stderr
    );
    assert!(
        stderr.contains("a.md") && stderr.contains("b.md"),
        "should name both duplicates: {}",
        stderr
    );
}

#[test]
fn duplicate_files_passes_distinct() {
    let temp_dir = setup_project(&[("a.md", "content one\n"), ("b.md", "content two\n")]);
    let output = run_rsconstruct_with_env(temp_dir.path(), &["build"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "build should succeed with distinct files: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A duplicate of an old, unchanged file added later must still be detected:
/// the whole-set product's input list changes, invalidating the cache.
#[test]
fn duplicate_files_detects_duplicate_added_incrementally() {
    let temp_dir = setup_project(&[("a.md", "original content\n"), ("b.md", "other content\n")]);
    let p = temp_dir.path();

    let first = run_rsconstruct_with_env(p, &["build"], &[("NO_COLOR", "1")]);
    assert!(
        first.status.success(),
        "initial build should pass: stderr={}",
        String::from_utf8_lossy(&first.stderr)
    );

    // Add a copy of an existing, unchanged file
    fs::write(p.join("copy.md"), "original content\n").unwrap();

    let second = run_rsconstruct_with_env(p, &["build"], &[("NO_COLOR", "1")]);
    assert!(
        !second.status.success(),
        "rebuild should detect the new duplicate"
    );
    let stderr = String::from_utf8_lossy(&second.stderr);
    assert!(
        stderr.contains("a.md") && stderr.contains("copy.md"),
        "should name the duplicate pair: {}",
        stderr
    );
}

fn assert_no_duplicates(output: &std::process::Output, what: &str) {
    assert!(
        output.status.success(),
        "{what} must not be reported as a duplicate: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A scanned file listed again in `dep_inputs` is one input, not a
/// duplicate of itself.
#[test]
fn duplicate_files_dep_input_also_scanned_is_not_its_own_duplicate() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let p = temp_dir.path();
    fs::write(
        p.join("rsconstruct.toml"),
        "[processor.checker.duplicate_files]\nsrc_dirs = [\".\"]\ndep_inputs = [\"a.md\"]\n",
    )
    .unwrap();
    fs::write(p.join("a.md"), "content one\n").unwrap();
    fs::write(p.join("b.md"), "content two\n").unwrap();

    let output = run_rsconstruct_with_env(p, &["build"], &[("NO_COLOR", "1")]);
    assert_no_duplicates(&output, "a scanned file that is also a dep_input");
}

/// A dependency is not a candidate: a `dep_inputs` file with the same
/// content as a scanned file is not reported.
#[test]
fn duplicate_files_dep_input_with_same_content_is_not_a_duplicate() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let p = temp_dir.path();
    fs::write(
        p.join("rsconstruct.toml"),
        "[processor.checker.duplicate_files]\nsrc_dirs = [\"src\"]\ndep_inputs = [\"template.txt\"]\n",
    )
    .unwrap();
    fs::create_dir_all(p.join("src")).unwrap();
    fs::write(p.join("src/a.md"), "same content\n").unwrap();
    fs::write(p.join("template.txt"), "same content\n").unwrap();

    let output = run_rsconstruct_with_env(p, &["build"], &[("NO_COLOR", "1")]);
    assert_no_duplicates(&output, "a dep_input with a scanned file's content");
}

/// The same for a `dep_auto` file: tracked as a dependency, never compared.
#[test]
fn duplicate_files_dep_auto_with_same_content_is_not_a_duplicate() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let p = temp_dir.path();
    fs::write(
        p.join("rsconstruct.toml"),
        "[processor.checker.duplicate_files]\nsrc_dirs = [\"src\"]\ndep_auto = [\"notes.txt\"]\n",
    )
    .unwrap();
    fs::create_dir_all(p.join("src")).unwrap();
    fs::write(p.join("src/a.md"), "same content\n").unwrap();
    fs::write(p.join("notes.txt"), "same content\n").unwrap();

    let output = run_rsconstruct_with_env(p, &["build"], &[("NO_COLOR", "1")]);
    assert_no_duplicates(&output, "a dep_auto file with a scanned file's content");
}

/// Dependencies still do their job: editing a `dep_inputs` file re-runs
/// the comparison, and the scanned duplicates are still found.
#[test]
fn duplicate_files_dep_input_edit_rechecks_scanned_files() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let p = temp_dir.path();
    fs::write(
        p.join("rsconstruct.toml"),
        "[processor.checker.duplicate_files]\nsrc_dirs = [\"src\"]\ndep_inputs = [\"template.txt\"]\n",
    )
    .unwrap();
    fs::create_dir_all(p.join("src")).unwrap();
    fs::write(p.join("src/a.md"), "same content\n").unwrap();
    fs::write(p.join("src/b.md"), "same content\n").unwrap();
    fs::write(p.join("template.txt"), "template\n").unwrap();

    let output = run_rsconstruct_with_env(p, &["build"], &[("NO_COLOR", "1")]);
    assert!(
        !output.status.success(),
        "scanned duplicates must still be found"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("src/a.md")
            && stderr.contains("src/b.md")
            && !stderr.contains("template.txt"),
        "the duplicates are the scanned pair, not the dependency: {stderr}"
    );
}
