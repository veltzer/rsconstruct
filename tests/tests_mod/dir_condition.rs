//! `enabled_if_dir_exists`: a processor runs only on a machine where the
//! named directory exists, is reported as skipped elsewhere, and is then
//! off exactly as `enabled = false` would make it.

use crate::common::{
    run_rsconstruct, run_rsconstruct_with_env, setup_project_with_config, write_file,
};
use tempfile::TempDir;

fn tera_config(dir: &str) -> String {
    format!(
        "[processor.generator.tera]\nsrc_dirs = [\"tera.templates\"]\nenabled_if_dir_exists = \"{dir}\"\n"
    )
}

fn assert_success(output: &std::process::Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    assert!(
        output.status.success(),
        "command failed:\n{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    stdout
}

#[test]
fn processor_runs_when_dir_exists() {
    let marker = TempDir::new().unwrap();
    let temp_dir = setup_project_with_config(&tera_config(&marker.path().display().to_string()));
    let project = temp_dir.path();
    write_file(project, "tera.templates/gen.txt.tera", "generated");

    let stdout = assert_success(&run_rsconstruct(project, &["build"]));
    assert!(project.join("gen.txt").exists(), "tera did not run");
    assert!(
        !stdout.contains("skipped"),
        "nothing should be skipped: {stdout}"
    );
}

#[test]
fn processor_is_skipped_and_reported_when_dir_missing() {
    let marker = TempDir::new().unwrap();
    let missing = marker.path().join("not-here").display().to_string();
    let temp_dir = setup_project_with_config(&tera_config(&missing));
    let project = temp_dir.path();
    write_file(project, "tera.templates/gen.txt.tera", "generated");

    let stdout = assert_success(&run_rsconstruct(project, &["build"]));
    assert!(
        !project.join("gen.txt").exists(),
        "tera ran although {missing} is missing"
    );
    assert!(
        stdout.contains(&format!(
            "processor.generator.tera skipped: {missing} does not exist"
        )),
        "the build must say why the processor did not run: {stdout}"
    );

    let used = assert_success(&run_rsconstruct(project, &["processor", "used"]));
    assert!(
        used.contains(&format!(
            "Skipped on this machine: processor.generator.tera ({missing} does not exist)"
        )),
        "processor used must show the skipped processor: {used}"
    );
}

#[test]
fn tilde_expands_to_home() {
    let home = TempDir::new().unwrap();
    std::fs::create_dir_all(home.path().join("synced")).unwrap();
    let temp_dir = setup_project_with_config(&tera_config("~/synced"));
    let project = temp_dir.path();
    write_file(project, "tera.templates/gen.txt.tera", "generated");

    let home_str = home.path().display().to_string();
    assert_success(&run_rsconstruct_with_env(
        project,
        &["build"],
        &[("HOME", &home_str)],
    ));
    assert!(
        project.join("gen.txt").exists(),
        "~/synced under HOME should exist"
    );
}

/// A skipped processor is off like `enabled = false`, so the checks that
/// exempt disabled stanzas exempt it too: a `src_dirs` entry that exists
/// only on the machine the processor is meant for is not an error elsewhere.
#[test]
fn skipped_processor_is_exempt_from_missing_src_dirs() {
    let marker = TempDir::new().unwrap();
    let missing = marker.path().join("not-here").display().to_string();
    let temp_dir = setup_project_with_config(&format!(
        "[processor.generator.tera]\nsrc_dirs = [\"only-on-that-machine\"]\nenabled_if_dir_exists = \"{missing}\"\n"
    ));
    assert_success(&run_rsconstruct(temp_dir.path(), &["build"]));
}

/// `enabled = false` wins: a processor already off is not reported as
/// waiting for its directory.
#[test]
fn explicitly_disabled_processor_is_not_reported_as_skipped() {
    let marker = TempDir::new().unwrap();
    let missing = marker.path().join("not-here").display().to_string();
    let temp_dir =
        setup_project_with_config(&format!("{}enabled = false\n", tera_config(&missing)));
    let project = temp_dir.path();
    write_file(project, "tera.templates/gen.txt.tera", "generated");
    let stdout = assert_success(&run_rsconstruct(project, &["build"]));
    assert!(!stdout.contains("skipped"), "{stdout}");
}
