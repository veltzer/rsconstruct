//! `clean all -p`: a processor's outputs and cache entries go together, so
//! the next build runs its products again; `clean outputs` keeps the cache
//! and the next build restores. Both take the `build -p` selectors.

use crate::common::*;
use tempfile::TempDir;

fn project() -> TempDir {
    let temp_dir = setup_test_project();
    write_file(temp_dir.path(), "tera.templates/hello.txt.tera", "hello");
    let build = run_rsconstruct(temp_dir.path(), &["build"]);
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
    assert!(temp_dir.path().join("hello.txt").exists());
    temp_dir
}

/// Outputs and cache entry gone, and the next build builds rather than
/// restores. The selector may drop the `processor.` prefix, as the tables
/// print names.
#[test]
fn clean_all_of_a_processor_forces_a_rebuild() {
    let dir = project();
    let out = run_rsconstruct(dir.path(), &["clean", "all", "-p", "generator.tera"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("1 file(s) and 1 cache entry removed"),
        "{stdout}"
    );
    assert!(!dir.path().join("hello.txt").exists());
    assert!(
        dir.path().join(".rsconstruct").exists(),
        "the rest of the cache stays"
    );

    let result = run_rsconstruct_json(dir.path(), &["build"]);
    assert!(result.exit_success);
    assert_eq!(result.success, 1, "the product must be built again");
    assert_eq!(result.restored, 0, "nothing to restore from: {result:?}");
}

/// The control: `clean outputs` keeps the cache and the next build restores.
#[test]
fn clean_outputs_keeps_the_cache() {
    let dir = project();
    let out = run_rsconstruct(dir.path(), &["clean", "outputs", "-p", "@generator"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!dir.path().join("hello.txt").exists());
    let result = run_rsconstruct_json(dir.path(), &["build"]);
    assert_eq!(result.restored, 1, "{result:?}");
    assert_eq!(result.success, 0, "{result:?}");
}

/// `--dry-run` reports what would go and removes nothing.
#[test]
fn clean_all_dry_run_removes_nothing() {
    let dir = project();
    let out = run_rsconstruct(
        dir.path(),
        &["clean", "all", "-p", "@generator", "--dry-run"],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Would remove 1 output file(s), 0 output dir(s) and 1 cache entry"),
        "{stdout}"
    );
    assert!(dir.path().join("hello.txt").exists());
    let result = run_rsconstruct_json(dir.path(), &["build"]);
    assert_eq!(result.skipped, 1, "still cached and present: {result:?}");
}

/// An unknown selector is a config error that lists the processors, as in
/// `build -p`, and removes nothing.
#[test]
fn clean_all_rejects_unknown_processors() {
    let dir = project();
    let out = run_rsconstruct(dir.path(), &["clean", "all", "-p", "nope"]);
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Unknown processor(s)") && stderr.contains("processor.generator.tera"),
        "{stderr}"
    );
    assert!(dir.path().join("hello.txt").exists(), "nothing was removed");
}

/// `build -p` accepts the prefix-less name too, so a name copied from a
/// table selects the processor.
#[test]
fn build_accepts_prefixless_processor_names() {
    let dir = project();
    let result = run_rsconstruct_json(dir.path(), &["build", "-p", "generator.tera"]);
    assert!(result.exit_success, "{result:?}");
    assert_eq!(result.total_products, 1, "{result:?}");
}

/// Without `-p`, `clean all` is the whole reset: the output and cache
/// directories go; `--dry-run` names them and keeps them.
#[test]
fn clean_all_without_processors_removes_the_directories() {
    let dir = project();
    let out = run_rsconstruct(dir.path(), &["clean", "all", "--dry-run"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Would remove .rsconstruct/"), "{stdout}");
    assert!(dir.path().join(".rsconstruct").exists());

    let out = run_rsconstruct(dir.path(), &["clean", "all"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!dir.path().join(".rsconstruct").exists());
}
