mod common;

use std::fs;
use common::{setup_test_project, run_rsb, run_rsb_with_env};

#[test]
fn clean_command() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create and build a template
    fs::write(
        project_path.join("config/clean_test.py"),
        "test = 'clean'"
    ).expect("Failed to write config");

    fs::write(
        project_path.join("templates/cleanme.txt.tera"),
        "{% set c = load_python(path='config/clean_test.py') %}{{ c.test }}"
    ).expect("Failed to write template");

    // Build
    let build_output = run_rsb(project_path, &["build"]);
    assert!(build_output.status.success());

    // Verify files exist
    assert!(project_path.join("cleanme.txt").exists());
    assert!(project_path.join(".rsb/index.json").exists());

    // Clean
    let clean_output = run_rsb(project_path, &["clean"]);
    assert!(clean_output.status.success());

    // Verify build outputs are removed but cache is preserved
    assert!(!project_path.join("cleanme.txt").exists());
    assert!(project_path.join(".rsb").exists());
}

#[test]
fn force_rebuild() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create template
    fs::write(
        project_path.join("config/force.py"),
        "mode = 'force'"
    ).expect("Failed to write config");

    fs::write(
        project_path.join("templates/force.txt.tera"),
        "{% set c = load_python(path='config/force.py') %}Mode: {{ c.mode }}"
    ).expect("Failed to write template");

    // First build
    run_rsb(project_path, &["build"]);

    // Force rebuild
    let output = run_rsb_with_env(project_path, &["build", "--force"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("[template] Processing:"));
    assert!(!stdout.contains("Skipping (unchanged)"));
}

#[test]
fn no_color_env() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create a sleep file so there's something to process
    fs::create_dir_all(project_path.join("sleep")).unwrap();
    fs::write(project_path.join("sleep/color_test.sleep"), "0.01").unwrap();
    fs::write(
        project_path.join("rsb.toml"),
        "[processor]\nenabled = [\"sleep\"]\n"
    ).unwrap();

    // Run with NO_COLOR set
    let output = run_rsb_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    // ANSI escape codes start with \x1b[
    assert!(!stdout.contains("\x1b["), "Output should not contain ANSI escape codes when NO_COLOR is set");
}

#[test]
fn timings_flag() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create a sleep file
    fs::create_dir_all(project_path.join("sleep")).unwrap();
    fs::write(project_path.join("sleep/timing_test.sleep"), "0.01").unwrap();
    fs::write(
        project_path.join("rsb.toml"),
        "[processor]\nenabled = [\"sleep\"]\n"
    ).unwrap();

    // Run with --timings
    let output = run_rsb_with_env(project_path, &["build", "--timings"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should contain timing information
    assert!(stdout.contains("Timing:"), "Output should contain 'Timing:' header");
    assert!(stdout.contains("Total:"), "Output should contain 'Total:' line");
}

#[test]
fn no_timings_by_default() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create a sleep file
    fs::create_dir_all(project_path.join("sleep")).unwrap();
    fs::write(project_path.join("sleep/no_timing.sleep"), "0.01").unwrap();
    fs::write(
        project_path.join("rsb.toml"),
        "[processor]\nenabled = [\"sleep\"]\n"
    ).unwrap();

    // Run without --timings (and without --verbose)
    let output = run_rsb(project_path, &["build"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should NOT contain timing information
    assert!(!stdout.contains("Timing:"), "Output should not contain timing info without --timings flag");
    assert!(!stdout.contains("Total:"), "Output should not contain total timing without --timings flag");
}

#[test]
fn keep_going_continues_after_failure() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create sleep directory with one bad file and one good file
    fs::create_dir_all(project_path.join("sleep")).unwrap();
    fs::write(project_path.join("sleep/bad.sleep"), "not_a_number").unwrap();
    fs::write(project_path.join("sleep/good.sleep"), "0.01").unwrap();
    fs::write(
        project_path.join("rsb.toml"),
        "[processor]\nenabled = [\"sleep\"]\n"
    ).unwrap();

    // Run with --keep-going
    let output = run_rsb_with_env(project_path, &["build", "--keep-going"], &[("NO_COLOR", "1")]);

    // Should exit non-zero because of the failure
    assert!(!output.status.success(), "Build should fail with bad sleep file");

    // The good sleep file should still have been processed
    let good_stub = project_path.join("out/sleep/good.done");
    assert!(good_stub.exists(), "Good sleep file should still be processed with --keep-going");
}

#[test]
fn keep_going_short_flag() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create sleep directory with one bad file
    fs::create_dir_all(project_path.join("sleep")).unwrap();
    fs::write(project_path.join("sleep/bad_k.sleep"), "invalid").unwrap();
    fs::write(
        project_path.join("rsb.toml"),
        "[processor]\nenabled = [\"sleep\"]\n"
    ).unwrap();

    // Run with -k (short form)
    let output = run_rsb_with_env(project_path, &["build", "-k"], &[("NO_COLOR", "1")]);

    // Should exit non-zero since the sleep file has invalid content
    assert!(!output.status.success(), "Build should fail with bad sleep file");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should contain error reporting in stdout or stderr
    let combined = format!("{}{}", stdout, stderr);
    assert!(combined.contains("error") || combined.contains("Error"),
        "Should report errors: stdout={}, stderr={}", stdout, stderr);
}

#[test]
fn independent_products_cached_after_failure() {
    // When one product fails (without --keep-going), independent products
    // should still be processed and cached, so the next build only needs
    // to re-process the previously-failing product.
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    fs::create_dir_all(project_path.join("sleep")).unwrap();
    // "bad" sorts before "good" alphabetically, so it will be processed first
    fs::write(project_path.join("sleep/bad.sleep"), "not_a_number").unwrap();
    fs::write(project_path.join("sleep/good.sleep"), "0.01").unwrap();
    fs::write(
        project_path.join("rsb.toml"),
        "[processor]\nenabled = [\"sleep\"]\n"
    ).unwrap();

    // First build — should fail because of bad.sleep
    let output1 = run_rsb_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(!output1.status.success(), "Build should fail with bad sleep file");

    // But the good.sleep file should have been processed and cached
    let good_stub = project_path.join("out/sleep/good.done");
    assert!(good_stub.exists(),
        "Good sleep file should still be processed even without --keep-going");

    // Now fix the bad file
    fs::write(project_path.join("sleep/bad.sleep"), "0.01").unwrap();

    // Second build — good.sleep should be skipped (cached), only bad.sleep processed
    let output2 = run_rsb_with_env(project_path, &["build", "--verbose"], &[("NO_COLOR", "1")]);
    assert!(output2.status.success(),
        "Second build should succeed after fixing bad file: stdout={}, stderr={}",
        String::from_utf8_lossy(&output2.stdout),
        String::from_utf8_lossy(&output2.stderr));

    let stdout2 = String::from_utf8_lossy(&output2.stdout);
    // good.sleep should be skipped (unchanged/cached from first build)
    assert!(stdout2.contains("Skipping (unchanged):") || stdout2.contains("Restored from cache:"),
        "Good sleep file should be skipped on second build: {}", stdout2);
    // bad.sleep should be re-processed
    assert!(stdout2.contains("[sleep] Processing:"),
        "Fixed bad sleep file should be processed on second build: {}", stdout2);
}

#[test]
fn independent_products_cached_after_failure_parallel() {
    // Same test as above but with parallel execution
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    fs::create_dir_all(project_path.join("sleep")).unwrap();
    fs::write(project_path.join("sleep/bad.sleep"), "not_a_number").unwrap();
    fs::write(project_path.join("sleep/good.sleep"), "0.01").unwrap();
    fs::write(
        project_path.join("rsb.toml"),
        "[processor]\nenabled = [\"sleep\"]\n\n[build]\nparallel = 2\n"
    ).unwrap();

    // First build — should fail
    let output1 = run_rsb_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(!output1.status.success(), "Build should fail with bad sleep file");

    // Good file should still be processed
    let good_stub = project_path.join("out/sleep/good.done");
    assert!(good_stub.exists(),
        "Good sleep file should still be processed in parallel mode even without --keep-going");

    // Fix the bad file
    fs::write(project_path.join("sleep/bad.sleep"), "0.01").unwrap();

    // Second build — good.sleep should be skipped
    let output2 = run_rsb_with_env(project_path, &["build", "--verbose"], &[("NO_COLOR", "1")]);
    assert!(output2.status.success(),
        "Second build should succeed after fixing bad file: stdout={}, stderr={}",
        String::from_utf8_lossy(&output2.stdout),
        String::from_utf8_lossy(&output2.stderr));

    let stdout2 = String::from_utf8_lossy(&output2.stdout);
    assert!(stdout2.contains("Skipping (unchanged):") || stdout2.contains("Restored from cache:"),
        "Good sleep file should be skipped on second build (parallel): {}", stdout2);
    assert!(stdout2.contains("[sleep] Processing:"),
        "Fixed bad sleep file should be processed on second build (parallel): {}", stdout2);
}

#[test]
fn deterministic_build_order() {
    // Run two separate builds with multiple sleep files and verify
    // that the processing order is identical both times.
    let outputs: Vec<Vec<String>> = (0..2).map(|_| {
        let temp_dir = setup_test_project();
        let project_path = temp_dir.path();

        fs::create_dir_all(project_path.join("sleep")).unwrap();
        // Create several sleep files with distinct names
        for name in &["zebra", "alpha", "mango", "banana", "cherry"] {
            fs::write(
                project_path.join(format!("sleep/{}.sleep", name)),
                "0.01"
            ).unwrap();
        }

        fs::write(
            project_path.join("rsb.toml"),
            "[processor]\nenabled = [\"sleep\"]\n"
        ).unwrap();

        let output = run_rsb_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
        assert!(output.status.success(),
            "Build failed: {}",
            String::from_utf8_lossy(&output.stderr));

        // Extract the target name from "[sleep] Processing: <name>" lines
        let stdout = String::from_utf8_lossy(&output.stdout);
        let processing_names: Vec<String> = stdout
            .lines()
            .filter(|l| l.contains("[sleep] Processing:"))
            .filter_map(|l| {
                l.split("Processing:").nth(1).map(|s| s.trim().to_string())
            })
            .collect();
        assert_eq!(processing_names.len(), 5, "Should process all 5 sleep files: {}", stdout);
        processing_names
    }).collect();

    assert_eq!(outputs[0], outputs[1],
        "Build order must be deterministic across runs.\nFirst:  {:?}\nSecond: {:?}",
        outputs[0], outputs[1]);
}
