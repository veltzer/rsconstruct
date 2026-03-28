use std::fs;
use crate::common::{setup_test_project, run_rsconstruct, run_rsconstruct_with_env, run_rsconstruct_json};

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
        project_path.join("tera.templates/cleanme.txt.tera"),
        "{% set c = load_python(path='config/clean_test.py') %}{{ c.test }}"
    ).expect("Failed to write template");

    // Build
    let build_output = run_rsconstruct(project_path, &["build"]);
    assert!(build_output.status.success());

    // Verify files exist
    assert!(project_path.join("cleanme.txt").exists());
    assert!(project_path.join(".rsconstruct/db.redb").exists());

    // Clean
    let clean_output = run_rsconstruct(project_path, &["clean", "outputs"]);
    assert!(clean_output.status.success());

    // Verify build outputs are removed but cache is preserved
    assert!(!project_path.join("cleanme.txt").exists());
    assert!(project_path.join(".rsconstruct").exists());
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
        project_path.join("tera.templates/force.txt.tera"),
        "{% set c = load_python(path='config/force.py') %}Mode: {{ c.mode }}"
    ).expect("Failed to write template");

    // First build
    let first_build = run_rsconstruct(project_path, &["build"]);
    assert!(first_build.status.success(), "First build failed: {}", String::from_utf8_lossy(&first_build.stderr));

    // Force rebuild - should process, not skip
    let result = run_rsconstruct_json(project_path, &["build", "--force"]);
    assert!(result.exit_success);
    assert_eq!(result.success, 1, "Should have 1 successful build");
    assert_eq!(result.skipped, 0, "Should not skip anything with --force");
}

#[test]
fn no_color_env() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create a template so there's something to process
    fs::write(
        project_path.join("tera.templates/color_test.txt.tera"),
        "hello"
    ).unwrap();

    // Run with NO_COLOR set
    let output = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    // ANSI escape codes start with \x1b[
    assert!(!stdout.contains("\x1b["), "Output should not contain ANSI escape codes when NO_COLOR is set");
}

#[test]
fn timings_flag() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create a template
    fs::write(
        project_path.join("tera.templates/timing_test.txt.tera"),
        "hello"
    ).unwrap();

    // Run with --timings
    let output = run_rsconstruct_with_env(project_path, &["build", "--timings"], &[("NO_COLOR", "1")]);
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

    // Create a template
    fs::write(
        project_path.join("tera.templates/no_timing.txt.tera"),
        "hello"
    ).unwrap();

    // Run without --timings (and without --verbose)
    let output = run_rsconstruct(project_path, &["build"]);
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

    // Create one bad template and one good template
    fs::write(project_path.join("tera.templates/bad.txt.tera"), "{{ invalid").unwrap();
    fs::write(project_path.join("tera.templates/good.txt.tera"), "hello").unwrap();

    // Run with --keep-going
    let output = run_rsconstruct_with_env(project_path, &["build", "-v", "--keep-going"], &[("NO_COLOR", "1")]);

    // Should exit non-zero because of the failure
    assert!(!output.status.success(), "Build should fail with bad template");

    // The good template should still have been processed (verify via output)
    let stdout = String::from_utf8_lossy(&output.stdout);
    // With --keep-going, both files should be attempted to be processed
    assert!(stdout.contains("Processing:"), "Files should be processed with --keep-going");
}

#[test]
fn keep_going_short_flag() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create one bad template
    fs::write(project_path.join("tera.templates/bad_k.txt.tera"), "{{ invalid").unwrap();

    // Run with -k (short form)
    let output = run_rsconstruct_with_env(project_path, &["build", "-k"], &[("NO_COLOR", "1")]);

    // Should exit non-zero since the template has invalid content
    assert!(!output.status.success(), "Build should fail with bad template");
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

    // "aaa" sorts before "zzz" alphabetically, so it will be processed first
    fs::write(project_path.join("tera.templates/aaa.txt.tera"), "{{ invalid").unwrap();
    fs::write(project_path.join("tera.templates/zzz.txt.tera"), "hello").unwrap();

    // Build should fail on aaa.txt.tera and stop
    let result = run_rsconstruct_json(project_path, &["build"]);
    assert!(!result.exit_success, "Build should fail with bad template");
    assert_eq!(result.failed, 1, "Should have exactly 1 failure");
    // zzz.txt should NOT be processed because we stop on first error
    assert!(!result.has_product("zzz.txt", "success"),
        "Second file should NOT be processed after first error");
}

#[test]
fn keep_going_continues_after_error() {
    // With --keep-going, independent products should still be processed.
    // Use an undefined variable (runtime error) rather than a parse error,
    // because Tera loads all templates at once and a parse error poisons them all.
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    fs::write(project_path.join("tera.templates/bad.txt.tera"), "{{ undefined_var }}").unwrap();
    fs::write(project_path.join("tera.templates/good.txt.tera"), "hello").unwrap();

    // First build with --keep-going — should fail but process all files
    let result1 = run_rsconstruct_json(project_path, &["build", "--keep-going"]);
    assert!(!result1.exit_success, "Build should fail with bad template");
    assert_eq!(result1.failed, 1, "Should have 1 failure");
    assert_eq!(result1.success, 1, "Should have 1 success (good.txt)");
    assert!(result1.has_product("good.txt", "success"),
        "Good template should be processed with --keep-going");

    // Fix the bad file
    fs::write(project_path.join("tera.templates/bad.txt.tera"), "fixed").unwrap();

    // Second build — good.txt should be skipped (cached)
    let result2 = run_rsconstruct_json(project_path, &["build"]);
    assert!(result2.exit_success, "Second build should succeed");
    assert_eq!(result2.skipped, 1, "Good template should be skipped (cached)");
    assert_eq!(result2.success, 1, "Bad template (now fixed) should be processed");
}

#[test]
fn parallel_build_with_j_flag() {
    // Verify -j flag enables parallel execution and all products are built
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    for name in &["alpha", "beta", "gamma", "delta"] {
        fs::write(
            project_path.join(format!("tera.templates/{}.txt.tera", name)),
            "hello"
        ).unwrap();
    }

    let result = run_rsconstruct_json(project_path, &["build", "-j2"]);
    assert!(result.exit_success, "Parallel build with -j2 should succeed");
    assert_eq!(result.success, 4, "Should process all 4 templates");
    assert_eq!(result.total_products, 4);
}

#[test]
fn parallel_keep_going_continues_after_failure() {
    // Verify --keep-going processes all independent products even when one fails
    // under parallel execution.
    // Use an undefined variable (runtime error) rather than a parse error,
    // because Tera loads all templates at once and a parse error poisons them all.
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    fs::write(project_path.join("tera.templates/aaa_bad.txt.tera"), "{{ undefined_var }}").unwrap();
    fs::write(project_path.join("tera.templates/good1.txt.tera"), "hello1").unwrap();
    fs::write(project_path.join("tera.templates/good2.txt.tera"), "hello2").unwrap();
    fs::write(project_path.join("tera.templates/good3.txt.tera"), "hello3").unwrap();
    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.tera]\n\n[build]\nparallel = 2\n"
    ).unwrap();

    let result = run_rsconstruct_json(project_path, &["build", "--keep-going"]);

    // Should fail overall
    assert!(!result.exit_success, "Build should fail with bad template even with --keep-going");
    assert_eq!(result.failed, 1, "Should have 1 failure");
    assert_eq!(result.success, 3, "All 3 good templates should be processed with --keep-going");
}

#[test]
fn parallel_builds_all_independent_products() {
    // Verify parallel config in rsconstruct.toml works and all products complete
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    for i in 0..8 {
        fs::write(
            project_path.join(format!("tera.templates/task_{:02}.txt.tera", i)),
            "hello"
        ).unwrap();
    }
    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.tera]\n\n[build]\nparallel = 4\n"
    ).unwrap();

    let result = run_rsconstruct_json(project_path, &["build"]);
    assert!(result.exit_success, "Parallel build with 8 products and 4 jobs should succeed");
    assert_eq!(result.success, 8, "Should process all 8 templates");
    assert_eq!(result.total_products, 8);

    // Incremental: second build should skip everything
    let result2 = run_rsconstruct_json(project_path, &["build"]);
    assert!(result2.exit_success);
    assert_eq!(result2.skipped, 8, "All 8 products should be skipped on second build");
}

#[test]
fn parallel_timings_flag() {
    // Verify --timings output works with parallel builds
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    for name in &["one", "two", "three"] {
        fs::write(
            project_path.join(format!("tera.templates/{}.txt.tera", name)),
            "hello"
        ).unwrap();
    }
    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.tera]\n\n[build]\nparallel = 2\n"
    ).unwrap();

    let output = run_rsconstruct_with_env(
        project_path, &["build", "--timings"], &[("NO_COLOR", "1")]
    );
    assert!(output.status.success(),
        "Parallel build with --timings should succeed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Timing:"), "Should contain 'Timing:' header in parallel mode");
    assert!(stdout.contains("Total:"), "Should contain 'Total:' line in parallel mode");

    // Should have timing entries
    let timing_lines = stdout.lines()
        .filter(|l| l.contains("[tera]") && l.contains("(0."))
        .count();
    assert!(timing_lines >= 1,
        "Should have at least one timing entry: {}", stdout);
}

#[test]
fn deterministic_build_order() {
    // Run two separate builds with multiple templates and verify
    // that the processing order is identical both times.
    let outputs: Vec<Vec<String>> = (0..2).map(|_| {
        let temp_dir = setup_test_project();
        let project_path = temp_dir.path();

        // Create several template files with distinct names
        for name in &["zebra", "alpha", "mango", "banana", "cherry"] {
            fs::write(
                project_path.join(format!("tera.templates/{}.txt.tera", name)),
                "hello"
            ).unwrap();
        }

        let output = run_rsconstruct_with_env(project_path, &["build", "-v"], &[("NO_COLOR", "1")]);
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
        assert_eq!(processing_names.len(), 5, "Should process all 5 templates: {}", stdout);
        processing_names
    }).collect();

    assert_eq!(outputs[0], outputs[1],
        "Build order must be deterministic across runs.\nFirst:  {:?}\nSecond: {:?}",
        outputs[0], outputs[1]);
}

/// Test that classify_products propagates dependency changes transitively.
/// Setup: tera generates step1.txt, a second tera template depends on step1.txt via extra_inputs.
/// When the first tera template changes, both products should be classified as needing
/// rebuild — not just the first product.
#[test]
fn classify_propagates_through_dependencies() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Phase 1: build with tera to create the output file
    fs::write(
        project_path.join("config/gen.py"),
        "val = 1"
    ).unwrap();
    fs::write(
        project_path.join("tera.templates/step1.txt.tera"),
        "{% set c = load_python(path='config/gen.py') %}step1={{ c.val }}"
    ).unwrap();

    let output1 = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output1.status.success(),
        "Phase 1 build failed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output1.stdout),
        String::from_utf8_lossy(&output1.stderr));
    assert!(project_path.join("step1.txt").exists(), "Tera should generate step1.txt");

    // Phase 2: add a second template with extra_inputs pointing to the first tera output
    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.tera]\nextra_inputs = [\"step1.txt\"]\n"
    ).unwrap();
    fs::write(
        project_path.join("tera.templates/step2.txt.tera"),
        "step2"
    ).unwrap();

    // Build both products
    let output2 = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output2.status.success(),
        "Phase 2 build failed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output2.stdout),
        String::from_utf8_lossy(&output2.stderr));

    // Verify everything is up-to-date
    let output3 = run_rsconstruct_with_env(
        project_path, &["build", "--stop-after", "classify"], &[("NO_COLOR", "1")]
    );
    assert!(output3.status.success());
    let stdout3 = String::from_utf8_lossy(&output3.stdout);
    assert!(stdout3.contains("2 up-to-date, 0 to restore, 0 to build"),
        "Both products should be up-to-date: {}", stdout3);

    // Wait so mtime differs
    std::thread::sleep(std::time::Duration::from_millis(100));

    // Modify the first tera template
    fs::write(
        project_path.join("tera.templates/step1.txt.tera"),
        "{% set c = load_python(path='config/gen.py') %}modified={{ c.val }}"
    ).unwrap();

    // Classify: both products should need work (tera rebuild + second rebuild/restore)
    let output4 = run_rsconstruct_with_env(
        project_path, &["build", "--stop-after", "classify"], &[("NO_COLOR", "1")]
    );
    assert!(output4.status.success());
    let stdout4 = String::from_utf8_lossy(&output4.stdout);
    assert!(stdout4.contains("0 up-to-date"),
        "No products should be up-to-date when root dependency changed: {}", stdout4);
}

#[test]
fn checker_and_generator_both_rebuild_on_shared_input_change() {
    // Regression test: when a checker and generator share the same input file,
    // modifying the input must cause BOTH to rebuild, not just the checker.
    // Bug scenario: checker runs first, updates the input hash in cache,
    // then generator sees matching hash and skips even though its output is stale.
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Enable both tera (generator) and script_check (checker) on .tera files
    fs::write(
        project_path.join("rsconstruct.toml"),
        concat!(
            "[processor.tera]\n",
            "\n",
            "[processor.script_check]\n",
            "scan_dir = \"tera.templates\"\n",
            "extensions = [\".tera\"]\n",
            "linter = \"true\"\n",
        ),
    ).unwrap();

    // Create a template
    fs::write(
        project_path.join("tera.templates/shared.txt.tera"),
        "version1",
    ).unwrap();

    // First build: both checker and generator should run
    let result1 = run_rsconstruct_json(project_path, &["build"]);
    assert!(result1.exit_success, "First build should succeed");
    assert!(result1.has_product("shared.txt", "success"),
        "Tera should process: {:?}", result1.products);
    assert_eq!(result1.failed, 0, "No failures expected");

    // Verify the output was created
    assert!(project_path.join("shared.txt").exists(),
        "Tera output should exist after first build");
    let content1 = fs::read_to_string(project_path.join("shared.txt")).unwrap();
    assert_eq!(content1, "version1");

    // Second build: everything should be skipped (no changes)
    let result2 = run_rsconstruct_json(project_path, &["build"]);
    assert!(result2.exit_success);
    assert_eq!(result2.skipped, result2.total_products,
        "All products should be skipped on second build (no changes)");

    // Wait for mtime to differ
    std::thread::sleep(std::time::Duration::from_millis(100));

    // Modify the shared input file
    fs::write(
        project_path.join("tera.templates/shared.txt.tera"),
        "version2",
    ).unwrap();

    // Third build: BOTH checker and generator must rebuild
    let result3 = run_rsconstruct_json(project_path, &["build"]);
    assert!(result3.exit_success, "Third build should succeed: {:?}", result3.errors);
    assert_eq!(result3.skipped, 0,
        "No products should be skipped after input change, got: {:?}", result3.products);
    assert!(result3.has_product("shared.txt", "success"),
        "Tera generator MUST rebuild after input change: {:?}", result3.products);

    // Verify the output was updated
    let content3 = fs::read_to_string(project_path.join("shared.txt")).unwrap();
    assert_eq!(content3, "version2",
        "Output should contain new content after rebuild");
}
