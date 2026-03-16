use std::fs;
use crate::common::{setup_test_project, run_rsconstruct, run_rsconstruct_with_env};

#[test]
fn dry_run_shows_build_actions() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create a sleep file
    fs::create_dir_all(project_path.join("sleep")).unwrap();
    fs::write(project_path.join("sleep/dry.sleep"), "0.01").unwrap();
    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor]\nenabled = [\"sleep\"]\n"
    ).unwrap();

    // Dry run before any build — should show BUILD
    let output = run_rsconstruct_with_env(project_path, &["build", "--dry-run"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("BUILD"), "Dry run should show BUILD for unbuilt product: {}", stdout);
    assert!(stdout.contains("Summary"), "Dry run should show Summary");

    // Checkers no longer create output files - verify no out/sleep directory
    assert!(!project_path.join("out/sleep").exists(), "Dry run should not create output directories");

    // Now do a real build
    let build = run_rsconstruct(project_path, &["build"]);
    assert!(build.status.success());
    // Checkers don't create stub files anymore

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

    fs::create_dir_all(project_path.join("sleep")).unwrap();
    fs::write(project_path.join("sleep/short.sleep"), "0.01").unwrap();
    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor]\nenabled = [\"sleep\"]\n"
    ).unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build", "-n"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(), "Short flag -n should work: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("BUILD"), "-n flag should show BUILD: {}", stdout);
    // Verify no output directory was created (dry-run should not execute anything)
    assert!(!project_path.join("out/sleep").exists(), "-n should not create output directories");
}

#[test]
fn dry_run_with_force() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    fs::create_dir_all(project_path.join("sleep")).unwrap();
    fs::write(project_path.join("sleep/force_dry.sleep"), "0.01").unwrap();
    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor]\nenabled = [\"sleep\"]\n"
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
