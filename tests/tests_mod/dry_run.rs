use std::fs;
use crate::common::{setup_test_project, run_rsconstruct, run_rsconstruct_with_env};

#[test]
fn dry_run_shows_build_actions() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create a template
    fs::write(
        project_path.join("tera.templates/dry.txt.tera"),
        "hello"
    ).unwrap();

    // Dry run before any build — should show BUILD
    let output = run_rsconstruct_with_env(project_path, &["build", "--dry-run"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("BUILD"), "Dry run should show BUILD for unbuilt product: {}", stdout);
    assert!(stdout.contains("Summary"), "Dry run should show Summary");

    // Verify no output file was created (dry-run should not execute anything)
    assert!(!project_path.join("dry.txt").exists(), "Dry run should not create output files");

    // Now do a real build
    let build = run_rsconstruct(project_path, &["build"]);
    assert!(build.status.success());

    // Dry run after build — should show SKIP (cache entry exists)
    let output2 = run_rsconstruct_with_env(project_path, &["build", "--dry-run"], &[("NO_COLOR", "1")]);
    assert!(output2.status.success());
    let stdout2 = String::from_utf8_lossy(&output2.stdout);
    assert!(stdout2.contains("SKIP"), "Dry run after build should show SKIP: {}", stdout2);
}

#[test]
fn dry_run_short_flag() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("tera.templates/short.txt.tera"),
        "hello"
    ).unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build", "-n"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(), "Short flag -n should work: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("BUILD"), "-n flag should show BUILD: {}", stdout);
    // Verify no output file was created (dry-run should not execute anything)
    assert!(!project_path.join("short.txt").exists(), "-n should not create output files");
}

#[test]
fn dry_run_with_force() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("tera.templates/force_dry.txt.tera"),
        "hello"
    ).unwrap();

    // Build first
    let build = run_rsconstruct(project_path, &["build"]);
    assert!(build.status.success());

    // Dry run with --force — should show BUILD even though up-to-date
    let output = run_rsconstruct_with_env(project_path, &["build", "--dry-run", "--force"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("BUILD"), "Dry run with --force should show BUILD: {}", stdout);
    assert!(!stdout.contains("SKIP"), "Dry run with --force should not show SKIP: {}", stdout);
}
