use std::fs;
use crate::common::{setup_test_project, run_rsb, run_rsb_with_env};

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
    assert!(project_path.join(".rsb/db").exists());

    // Clean
    let clean_output = run_rsb(project_path, &["clean", "outputs"]);
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
    assert!(stdout.contains("Processing:"));
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

    // The good sleep file should still have been processed (verify via output)
    let stdout = String::from_utf8_lossy(&output.stdout);
    // With --keep-going, both files should be attempted to be processed
    assert!(stdout.contains("Processing:"), "Files should be processed with --keep-going");
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
fn build_stops_on_first_error() {
    // Without --keep-going, build should stop immediately on first error
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    fs::create_dir_all(project_path.join("sleep")).unwrap();
    // "aaa" sorts before "zzz" alphabetically, so it will be processed first
    fs::write(project_path.join("sleep/aaa.sleep"), "not_a_number").unwrap();
    fs::write(project_path.join("sleep/zzz.sleep"), "0.01").unwrap();
    fs::write(
        project_path.join("rsb.toml"),
        "[processor]\nenabled = [\"sleep\"]\n"
    ).unwrap();

    // Build should fail on aaa.sleep and stop
    let output = run_rsb_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(!output.status.success(), "Build should fail with bad sleep file");

    let stdout = String::from_utf8_lossy(&output.stdout);
    // zzz.sleep should NOT be processed because we stop on first error
    assert!(!stdout.contains("zzz.sleep"),
        "Second file should NOT be processed after first error: {}", stdout);
}

#[test]
fn keep_going_continues_after_error() {
    // With --keep-going, independent products should still be processed
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    fs::create_dir_all(project_path.join("sleep")).unwrap();
    fs::write(project_path.join("sleep/bad.sleep"), "not_a_number").unwrap();
    fs::write(project_path.join("sleep/good.sleep"), "0.01").unwrap();
    fs::write(
        project_path.join("rsb.toml"),
        "[processor]\nenabled = [\"sleep\"]\n"
    ).unwrap();

    // First build with --keep-going — should fail but process all files
    let output1 = run_rsb_with_env(project_path, &["build", "--keep-going"], &[("NO_COLOR", "1")]);
    assert!(!output1.status.success(), "Build should fail with bad sleep file");

    let stdout1 = String::from_utf8_lossy(&output1.stdout);
    // Both files should have been processed
    assert!(stdout1.contains("good.sleep"),
        "Good sleep file should be processed with --keep-going: {}", stdout1);

    // Fix the bad file
    fs::write(project_path.join("sleep/bad.sleep"), "0.01").unwrap();

    // Second build — good.sleep should be skipped (cached)
    let output2 = run_rsb_with_env(project_path, &["build", "--verbose"], &[("NO_COLOR", "1")]);
    assert!(output2.status.success(),
        "Second build should succeed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output2.stdout),
        String::from_utf8_lossy(&output2.stderr));

    let stdout2 = String::from_utf8_lossy(&output2.stdout);
    assert!(stdout2.contains("Skipping (unchanged):"),
        "Good sleep file should be skipped on second build: {}", stdout2);
}

#[test]
fn parallel_build_with_j_flag() {
    // Verify -j flag enables parallel execution and all products are built
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    fs::create_dir_all(project_path.join("sleep")).unwrap();
    for name in &["alpha", "beta", "gamma", "delta"] {
        fs::write(
            project_path.join(format!("sleep/{}.sleep", name)),
            "0.01"
        ).unwrap();
    }
    fs::write(
        project_path.join("rsb.toml"),
        "[processor]\nenabled = [\"sleep\"]\n"
    ).unwrap();

    let output = run_rsb_with_env(project_path, &["build", "-j2"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(),
        "Parallel build with -j2 should succeed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr));

    // Checkers no longer create stub files - verify all were processed via output
    let stdout = String::from_utf8_lossy(&output.stdout);
    let processing_count = stdout.lines()
        .filter(|l| l.contains("Processing:"))
        .count();
    assert_eq!(processing_count, 4, "Should process all 4 sleep files: {}", stdout);
}

#[test]
fn parallel_keep_going_continues_after_failure() {
    // Verify --keep-going processes all independent products even when one fails
    // under parallel execution
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    fs::create_dir_all(project_path.join("sleep")).unwrap();
    fs::write(project_path.join("sleep/aaa_bad.sleep"), "not_a_number").unwrap();
    fs::write(project_path.join("sleep/good1.sleep"), "0.01").unwrap();
    fs::write(project_path.join("sleep/good2.sleep"), "0.01").unwrap();
    fs::write(project_path.join("sleep/good3.sleep"), "0.01").unwrap();
    fs::write(
        project_path.join("rsb.toml"),
        "[processor]\nenabled = [\"sleep\"]\n\n[build]\nparallel = 2\n"
    ).unwrap();

    let output = run_rsb_with_env(
        project_path, &["build", "--keep-going"], &[("NO_COLOR", "1")]
    );

    // Should fail overall
    assert!(!output.status.success(),
        "Build should fail with bad sleep file even with --keep-going");

    // Checkers no longer create stub files - verify via output that good files were processed
    let stdout = String::from_utf8_lossy(&output.stdout);
    let processing_count = stdout.lines()
        .filter(|l| l.contains("Processing:"))
        .count();
    // All 4 files should be attempted (3 good + 1 bad)
    assert!(processing_count >= 3,
        "At least 3 good sleep files should be processed with --keep-going in parallel: {}", stdout);
}

#[test]
fn parallel_builds_all_independent_products() {
    // Verify parallel config in rsb.toml works and all products complete
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    fs::create_dir_all(project_path.join("sleep")).unwrap();
    for i in 0..8 {
        fs::write(
            project_path.join(format!("sleep/task_{:02}.sleep", i)),
            "0.01"
        ).unwrap();
    }
    fs::write(
        project_path.join("rsb.toml"),
        "[processor]\nenabled = [\"sleep\"]\n\n[build]\nparallel = 4\n"
    ).unwrap();

    let output = run_rsb_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(),
        "Parallel build with 8 products and 4 jobs should succeed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr));

    // Checkers no longer create stub files - verify via output that all were processed
    let stdout = String::from_utf8_lossy(&output.stdout);
    let processing_count = stdout.lines()
        .filter(|l| l.contains("Processing:"))
        .count();
    assert_eq!(processing_count, 8, "Should process all 8 sleep files: {}", stdout);

    // Incremental: second build should skip everything
    let output2 = run_rsb_with_env(project_path, &["build", "--verbose"], &[("NO_COLOR", "1")]);
    assert!(output2.status.success());
    let stdout2 = String::from_utf8_lossy(&output2.stdout);
    let skip_count = stdout2.lines()
        .filter(|l| l.contains("Skipping (unchanged):"))
        .count();
    assert_eq!(skip_count, 8,
        "All 8 products should be skipped on second build: {}", stdout2);
}

#[test]
fn parallel_timings_flag() {
    // Verify --timings output works with parallel builds
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    fs::create_dir_all(project_path.join("sleep")).unwrap();
    for name in &["one", "two", "three"] {
        fs::write(
            project_path.join(format!("sleep/{}.sleep", name)),
            "0.01"
        ).unwrap();
    }
    fs::write(
        project_path.join("rsb.toml"),
        "[processor]\nenabled = [\"sleep\"]\n\n[build]\nparallel = 2\n"
    ).unwrap();

    let output = run_rsb_with_env(
        project_path, &["build", "--timings"], &[("NO_COLOR", "1")]
    );
    assert!(output.status.success(),
        "Parallel build with --timings should succeed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Timing:"), "Should contain 'Timing:' header in parallel mode");
    assert!(stdout.contains("Total:"), "Should contain 'Total:' line in parallel mode");

    // Should have timing entries (may be batched or individual depending on parallel execution)
    let timing_lines = stdout.lines()
        .filter(|l| l.contains("[sleep]") && l.contains("(0."))
        .count();
    assert!(timing_lines >= 1,
        "Should have at least one timing entry: {}", stdout);
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

        // Extract the target name from "Processing: <name>" lines
        let stdout = String::from_utf8_lossy(&output.stdout);
        let processing_names: Vec<String> = stdout
            .lines()
            .filter(|l| l.contains("Processing:"))
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
