mod common;

use std::fs;
use common::{setup_test_project, run_rsb, run_rsb_with_env};

#[test]
fn dry_run_shows_build_actions() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create a sleep file
    fs::create_dir_all(project_path.join("sleep")).unwrap();
    fs::write(project_path.join("sleep/dry.sleep"), "0.01").unwrap();
    fs::write(
        project_path.join("rsb.toml"),
        "[processor]\nenabled = [\"sleep\"]\n"
    ).unwrap();

    // Dry run before any build — should show BUILD
    let output = run_rsb_with_env(project_path, &["build", "--dry-run"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("BUILD"), "Dry run should show BUILD for unbuilt product: {}", stdout);
    assert!(stdout.contains("Summary"), "Dry run should show Summary");

    // Verify nothing was actually built
    assert!(!project_path.join("out/sleep/dry.done").exists(), "Dry run should not create output files");

    // Now do a real build
    let build = run_rsb(project_path, &["build"]);
    assert!(build.status.success());
    assert!(project_path.join("out/sleep/dry.done").exists());

    // Dry run after build — should show SKIP
    let output2 = run_rsb_with_env(project_path, &["build", "--dry-run"], &[("NO_COLOR", "1")]);
    assert!(output2.status.success());
    let stdout2 = String::from_utf8_lossy(&output2.stdout);
    assert!(stdout2.contains("SKIP"), "Dry run after build should show SKIP: {}", stdout2);

    // Delete output, dry run should show RESTORE
    fs::remove_file(project_path.join("out/sleep/dry.done")).unwrap();
    let output3 = run_rsb_with_env(project_path, &["build", "--dry-run"], &[("NO_COLOR", "1")]);
    assert!(output3.status.success());
    let stdout3 = String::from_utf8_lossy(&output3.stdout);
    assert!(stdout3.contains("RESTORE"), "Dry run with deleted output should show RESTORE: {}", stdout3);

    // Verify it still didn't actually restore
    assert!(!project_path.join("out/sleep/dry.done").exists(), "Dry run should not restore files");
}

#[test]
fn dry_run_short_flag() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    fs::create_dir_all(project_path.join("sleep")).unwrap();
    fs::write(project_path.join("sleep/short.sleep"), "0.01").unwrap();
    fs::write(
        project_path.join("rsb.toml"),
        "[processor]\nenabled = [\"sleep\"]\n"
    ).unwrap();

    let output = run_rsb_with_env(project_path, &["build", "-n"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(), "Short flag -n should work: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("BUILD"), "-n flag should show BUILD: {}", stdout);
    assert!(!project_path.join("out/sleep/short.done").exists(), "-n should not build");
}

#[test]
fn dry_run_with_force() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    fs::create_dir_all(project_path.join("sleep")).unwrap();
    fs::write(project_path.join("sleep/force_dry.sleep"), "0.01").unwrap();
    fs::write(
        project_path.join("rsb.toml"),
        "[processor]\nenabled = [\"sleep\"]\n"
    ).unwrap();

    // Build first
    let build = run_rsb(project_path, &["build"]);
    assert!(build.status.success());

    // Dry run with --force — should show BUILD even though up-to-date
    let output = run_rsb_with_env(project_path, &["build", "--dry-run", "--force"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("BUILD"), "Dry run with --force should show BUILD: {}", stdout);
    assert!(!stdout.contains("SKIP"), "Dry run with --force should not show SKIP: {}", stdout);
}
