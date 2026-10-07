use crate::common::{
    make_executable, run_rsconstruct, run_rsconstruct_with_env, setup_test_project,
};
use std::fs;
use std::path::Path;
use tempfile::TempDir;

/// A two-step chain: `first` turns `src/a.src` into `mid/a.mid`, `second`
/// turns that into `out/second/a.out`. Both tools copy their last two
/// arguments, so `args` can change without changing what they do.
fn copy_chain() -> TempDir {
    let temp_dir = TempDir::new().unwrap();
    let project = temp_dir.path();
    let bin_dir = project.join("toolbin");
    fs::create_dir_all(&bin_dir).unwrap();
    let tool = bin_dir.join("copy");
    fs::write(
        &tool,
        "#!/bin/sh\nwhile [ $# -gt 2 ]; do shift; done\ncp \"$1\" \"$2\"\n",
    )
    .unwrap();
    make_executable(&tool);
    fs::write(project.join("rsconstruct.toml"), chain_config("[]")).unwrap();
    fs::create_dir_all(project.join("src")).unwrap();
    fs::write(project.join("src/a.src"), "one\n").unwrap();
    temp_dir
}

fn chain_config(first_args: &str) -> String {
    format!(
        r#"[build]
hash_tool_versions = false

[processor.generator.generic.first]
command = "copy"
args = {first_args}
output_dir = "mid"
output_extension = "mid"
batch = false
src_extensions = [".src"]
src_dirs = ["src"]

[processor.generator.generic.second]
command = "copy"
output_dir = "out/second"
output_extension = "out"
batch = false
src_extensions = [".mid"]
src_dirs = ["mid"]
"#
    )
}

/// Run rsconstruct with `toolbin/` first on PATH; returns (success, stdout).
fn run(project: &Path, args: &[&str]) -> (bool, String) {
    let path_env = format!(
        "{}:{}",
        project.join("toolbin").display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let output = run_rsconstruct_with_env(project, args, &[("NO_COLOR", "1"), ("PATH", &path_env)]);
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr);
    (output.status.success(), format!("{stdout}{stderr}"))
}

/// A product whose input another product is about to rewrite builds in the
/// real build, so the dry run must say BUILD for it — not SKIP because its
/// input on disk still matches the cache.
#[test]
fn dry_run_predicts_rebuild_downstream_of_a_change() {
    let temp_dir = copy_chain();
    let project = temp_dir.path();
    assert!(run(project, &["build"]).0);
    fs::write(project.join("src/a.src"), "two\n").unwrap();

    let (ok, out) = run(project, &["build", "--dry-run", "--explain"]);
    assert!(ok, "{out}");
    assert!(
        out.contains("BUILD [processor.generator.generic.second] mid/a.mid"),
        "second must be predicted to build: {out}"
    );
    assert!(
        out.contains("a dependency will be rebuilt or restored"),
        "the reason names the dependency: {out}"
    );

    let (ok, out) = run(project, &["build"]);
    assert!(ok, "{out}");
    assert!(out.contains("2 built"), "the real build agrees: {out}");
}

/// The dry run plans exactly what `build` with the same flags would run.
#[test]
fn dry_run_honors_processor_filter() {
    let temp_dir = copy_chain();
    let project = temp_dir.path();
    let (ok, out) = run(
        project,
        &["build", "-n", "-p", "processor.generator.generic.first"],
    );
    assert!(ok, "{out}");
    assert!(out.contains("[processor.generator.generic.first]"), "{out}");
    assert!(
        !out.contains("[processor.generator.generic.second]"),
        "-p first must leave out second: {out}"
    );
}

/// A dry run must not advance the stored config baseline: the next real
/// build still reports the config change.
#[test]
fn dry_run_leaves_config_change_for_the_build() {
    let temp_dir = copy_chain();
    let project = temp_dir.path();
    assert!(run(project, &["build"]).0);
    fs::write(
        project.join("rsconstruct.toml"),
        chain_config(r#"["--v2"]"#),
    )
    .unwrap();

    let (ok, out) = run(project, &["build", "--dry-run"]);
    assert!(ok, "{out}");
    let (ok, out) = run(project, &["build"]);
    assert!(ok, "{out}");
    assert!(
        out.contains("Config changed for [processor.generator.generic.first]"),
        "the build after a dry run still sees the change: {out}"
    );
}

#[test]
fn dry_run_shows_build_actions() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create a template
    fs::write(project_path.join("tera.templates/dry.txt.tera"), "hello").unwrap();

    // Dry run before any build — should show BUILD
    let output =
        run_rsconstruct_with_env(project_path, &["build", "--dry-run"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("BUILD"),
        "Dry run should show BUILD for unbuilt product: {}",
        stdout
    );
    assert!(stdout.contains("Total"), "Dry run should show Total");

    // Verify no output file was created (dry-run should not execute anything)
    assert!(
        !project_path.join("dry.txt").exists(),
        "Dry run should not create output files"
    );

    // Now do a real build
    let build = run_rsconstruct(project_path, &["build"]);
    assert!(build.status.success());

    // Dry run after build — should show SKIP (cache entry exists)
    let output2 =
        run_rsconstruct_with_env(project_path, &["build", "--dry-run"], &[("NO_COLOR", "1")]);
    assert!(output2.status.success());
    let stdout2 = String::from_utf8_lossy(&output2.stdout);
    assert!(
        stdout2.contains("SKIP"),
        "Dry run after build should show SKIP: {}",
        stdout2
    );
}

#[test]
fn dry_run_short_flag() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    fs::write(project_path.join("tera.templates/short.txt.tera"), "hello").unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build", "-n"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "Short flag -n should work: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("BUILD"),
        "-n flag should show BUILD: {}",
        stdout
    );
    // Verify no output file was created (dry-run should not execute anything)
    assert!(
        !project_path.join("short.txt").exists(),
        "-n should not create output files"
    );
}

#[test]
fn dry_run_with_force() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("tera.templates/force_dry.txt.tera"),
        "hello",
    )
    .unwrap();

    // Build first
    let build = run_rsconstruct(project_path, &["build"]);
    assert!(build.status.success());

    // Dry run with --force — should show BUILD even though up-to-date
    let output = run_rsconstruct_with_env(
        project_path,
        &["build", "--dry-run", "--force"],
        &[("NO_COLOR", "1")],
    );
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("BUILD"),
        "Dry run with --force should show BUILD: {}",
        stdout
    );
    assert!(
        !stdout.contains("SKIP"),
        "Dry run with --force should not show SKIP: {}",
        stdout
    );
}
