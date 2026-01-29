use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

/// Helper to create a test project structure
fn setup_test_project() -> TempDir {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    // Create directories
    fs::create_dir_all(temp_dir.path().join("templates")).expect("Failed to create templates dir");
    fs::create_dir_all(temp_dir.path().join("config")).expect("Failed to create config dir");

    temp_dir
}

/// Helper to run rsb command in a directory
fn run_rsb(dir: &Path, args: &[&str]) -> std::process::Output {
    let rsb_path = env!("CARGO_BIN_EXE_rsb");
    Command::new(rsb_path)
        .current_dir(dir)
        .args(args)
        .output()
        .expect("Failed to execute rsb")
}

/// Helper to run rsb command with extra environment variables
fn run_rsb_with_env(dir: &Path, args: &[&str], env_vars: &[(&str, &str)]) -> std::process::Output {
    let rsb_path = env!("CARGO_BIN_EXE_rsb");
    let mut cmd = Command::new(rsb_path);
    cmd.current_dir(dir).args(args);
    for (key, val) in env_vars {
        cmd.env(key, val);
    }
    cmd.output().expect("Failed to execute rsb")
}

#[test]
fn test_template_to_file_translation() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create a Python config file
    let config_content = r#"
project_name = "TestProject"
version = "1.2.3"
author = "Test Author"
debug_mode = True
features = ["logging", "caching", "metrics"]
max_connections = 100
"#;
    fs::write(
        project_path.join("config/test_config.py"),
        config_content
    ).expect("Failed to write config file");

    // Create a template file
    let template_content = r#"{% set cfg = load_python(path="config/test_config.py") %}
# Generated configuration for {{ cfg.project_name }}
# Version: {{ cfg.version }}
# Author: {{ cfg.author }}

[settings]
project = "{{ cfg.project_name }}"
version = "{{ cfg.version }}"
debug = {{ cfg.debug_mode }}
max_connections = {{ cfg.max_connections }}

[features]
{% for feature in cfg.features -%}
{{ feature }} = enabled
{% endfor %}

# Build information
{% if cfg.debug_mode -%}
build_type = "debug"
optimization = 0
{% else -%}
build_type = "release"
optimization = 3
{% endif -%}
"#;
    fs::write(
        project_path.join("templates/app.config.tera"),
        template_content
    ).expect("Failed to write template file");

    // Run rsb build
    let output = run_rsb_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(), "rsb build failed: {}", String::from_utf8_lossy(&output.stderr));

    // Check that the output file was created
    let output_file = project_path.join("app.config");
    assert!(output_file.exists(), "Output file was not created");

    // Read and verify the generated file content
    let generated_content = fs::read_to_string(&output_file)
        .expect("Failed to read generated file");

    // Verify expected content in the generated file
    assert!(generated_content.contains("Generated configuration for TestProject"));
    assert!(generated_content.contains("Version: 1.2.3"));
    assert!(generated_content.contains("Author: Test Author"));
    assert!(generated_content.contains("debug = true"));
    assert!(generated_content.contains("max_connections = 100"));
    assert!(generated_content.contains("logging = enabled"));
    assert!(generated_content.contains("caching = enabled"));
    assert!(generated_content.contains("metrics = enabled"));
    assert!(generated_content.contains("build_type = \"debug\""));
    assert!(generated_content.contains("optimization = 0"));

    println!("Generated file content:\n{}", generated_content);
}

#[test]
fn test_incremental_build() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create a simple config and template
    fs::write(
        project_path.join("config/simple.py"),
        "name = 'SimpleTest'\ncount = 42"
    ).expect("Failed to write config");

    fs::write(
        project_path.join("templates/simple.txt.tera"),
        "{% set c = load_python(path='config/simple.py') %}Name: {{ c.name }}, Count: {{ c.count }}"
    ).expect("Failed to write template");

    // First build
    let output1 = run_rsb_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output1.status.success());
    let stdout1 = String::from_utf8_lossy(&output1.stdout);
    assert!(stdout1.contains("[template] Processing:"));

    // Second build (should skip unchanged template - use verbose to see skip message)
    let output2 = run_rsb_with_env(project_path, &["build", "--verbose"], &[("NO_COLOR", "1")]);
    assert!(output2.status.success());
    let stdout2 = String::from_utf8_lossy(&output2.stdout);
    assert!(stdout2.contains("[template] Skipping (unchanged):"));

    // Verify cache directory exists
    assert!(project_path.join(".rsb/index.json").exists());
}

#[test]
fn test_clean_command() {
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
fn test_force_rebuild() {
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
fn test_multiple_templates() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create shared config
    let config = "shared_name = 'MultiTest'\nshared_value = 123";
    fs::write(project_path.join("config/shared.py"), config).unwrap();

    // Create multiple templates
    fs::write(
        project_path.join("templates/first.txt.tera"),
        "{% set c = load_python(path='config/shared.py') %}First: {{ c.shared_name }}"
    ).unwrap();

    fs::write(
        project_path.join("templates/second.conf.tera"),
        "{% set c = load_python(path='config/shared.py') %}[config]\nname={{ c.shared_name }}\nvalue={{ c.shared_value }}"
    ).unwrap();

    fs::write(
        project_path.join("templates/third.json.tera"),
        r#"{% set c = load_python(path='config/shared.py') %}{"name": "{{ c.shared_name }}", "value": {{ c.shared_value }}}"#
    ).unwrap();

    // Build
    let output = run_rsb(project_path, &["build"]);
    assert!(output.status.success());

    // Check all files were created
    assert!(project_path.join("first.txt").exists());
    assert!(project_path.join("second.conf").exists());
    assert!(project_path.join("third.json").exists());

    // Verify content
    let first = fs::read_to_string(project_path.join("first.txt")).unwrap();
    assert_eq!(first.trim(), "First: MultiTest");

    let third = fs::read_to_string(project_path.join("third.json")).unwrap();
    assert!(third.contains(r#""name": "MultiTest""#));
    assert!(third.contains(r#""value": 123"#));
}

#[test]
fn test_cache_operations() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create a simple template
    fs::write(
        project_path.join("config/cache_test.py"),
        "value = 'cached'"
    ).unwrap();

    fs::write(
        project_path.join("templates/cached.txt.tera"),
        "{% set c = load_python(path='config/cache_test.py') %}{{ c.value }}"
    ).unwrap();

    // Build to populate cache
    let output = run_rsb(project_path, &["build"]);
    assert!(output.status.success());
    assert!(project_path.join("cached.txt").exists());
    assert!(project_path.join(".rsb/index.json").exists());
    assert!(project_path.join(".rsb/objects").exists());

    // Check cache size reports objects
    let size_output = run_rsb(project_path, &["cache", "size"]);
    assert!(size_output.status.success());
    let size_stdout = String::from_utf8_lossy(&size_output.stdout);
    assert!(size_stdout.contains("Cache size:"));
    assert!(size_stdout.contains("objects"));
    // Should have at least 1 object
    assert!(!size_stdout.contains("0 objects"), "Cache should have objects after build");

    // Delete the output file, then rebuild — should restore from cache
    fs::remove_file(project_path.join("cached.txt")).unwrap();
    assert!(!project_path.join("cached.txt").exists());

    let restore_output = run_rsb_with_env(project_path, &["build", "--verbose"], &[("NO_COLOR", "1")]);
    assert!(restore_output.status.success());
    let restore_stdout = String::from_utf8_lossy(&restore_output.stdout);
    assert!(restore_stdout.contains("Restored from cache:"));
    assert!(project_path.join("cached.txt").exists());

    // Verify restored content is correct
    let content = fs::read_to_string(project_path.join("cached.txt")).unwrap();
    assert_eq!(content.trim(), "cached");

    // Trim cache (nothing unreferenced, so 0 removed)
    let trim_output = run_rsb(project_path, &["cache", "trim"]);
    assert!(trim_output.status.success());
    let trim_stdout = String::from_utf8_lossy(&trim_output.stdout);
    assert!(trim_stdout.contains("0 unreferenced objects"));

    // Clear cache entirely
    let clear_output = run_rsb(project_path, &["cache", "clear"]);
    assert!(clear_output.status.success());
    assert!(!project_path.join(".rsb").exists());

    // Cache size after clear should be 0
    let size_after = run_rsb(project_path, &["cache", "size"]);
    assert!(size_after.status.success());
    let size_after_stdout = String::from_utf8_lossy(&size_after.stdout);
    assert!(size_after_stdout.contains("0 bytes"));
    assert!(size_after_stdout.contains("0 objects"));
}

#[test]
fn test_sleep_processor() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create sleep directory and a sleep file with a short duration
    fs::create_dir_all(project_path.join("sleep")).unwrap();
    fs::write(project_path.join("sleep/quick.sleep"), "0.1").unwrap();

    // Enable only sleep processor (disable template and lint to avoid needing their dirs)
    fs::write(
        project_path.join("rsb.toml"),
        "[processors]\nenabled = [\"sleep\"]\n"
    ).unwrap();

    // Build
    let output = run_rsb_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(), "rsb build failed: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("[sleep] Processing:"));

    // Verify stub file was created
    let stub_path = project_path.join("out/sleep/quick.done");
    assert!(stub_path.exists(), "Sleep stub file was not created");
    let stub_content = fs::read_to_string(&stub_path).unwrap();
    assert!(stub_content.contains("slept for 0.1 seconds"));

    // Second build should skip (incremental)
    let output2 = run_rsb_with_env(project_path, &["build", "--verbose"], &[("NO_COLOR", "1")]);
    assert!(output2.status.success());
    let stdout2 = String::from_utf8_lossy(&output2.stdout);
    assert!(stdout2.contains("[sleep] Skipping (unchanged):"));

    // Clean should remove stub files
    let clean_output = run_rsb(project_path, &["clean"]);
    assert!(clean_output.status.success());
    assert!(!stub_path.exists(), "Sleep stub should be removed after clean");
}

// ========== New tests for developer experience features ==========

#[test]
fn test_no_color_env() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create a sleep file so there's something to process
    fs::create_dir_all(project_path.join("sleep")).unwrap();
    fs::write(project_path.join("sleep/color_test.sleep"), "0.01").unwrap();
    fs::write(
        project_path.join("rsb.toml"),
        "[processors]\nenabled = [\"sleep\"]\n"
    ).unwrap();

    // Run with NO_COLOR set
    let output = run_rsb_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    // ANSI escape codes start with \x1b[
    assert!(!stdout.contains("\x1b["), "Output should not contain ANSI escape codes when NO_COLOR is set");
}

#[test]
fn test_timings_flag() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create a sleep file
    fs::create_dir_all(project_path.join("sleep")).unwrap();
    fs::write(project_path.join("sleep/timing_test.sleep"), "0.01").unwrap();
    fs::write(
        project_path.join("rsb.toml"),
        "[processors]\nenabled = [\"sleep\"]\n"
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
fn test_no_timings_by_default() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create a sleep file
    fs::create_dir_all(project_path.join("sleep")).unwrap();
    fs::write(project_path.join("sleep/no_timing.sleep"), "0.01").unwrap();
    fs::write(
        project_path.join("rsb.toml"),
        "[processors]\nenabled = [\"sleep\"]\n"
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
fn test_keep_going_continues_after_failure() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create sleep directory with one bad file and one good file
    fs::create_dir_all(project_path.join("sleep")).unwrap();
    fs::write(project_path.join("sleep/bad.sleep"), "not_a_number").unwrap();
    fs::write(project_path.join("sleep/good.sleep"), "0.01").unwrap();
    fs::write(
        project_path.join("rsb.toml"),
        "[processors]\nenabled = [\"sleep\"]\n"
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
fn test_keep_going_short_flag() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create sleep directory with one bad file
    fs::create_dir_all(project_path.join("sleep")).unwrap();
    fs::write(project_path.join("sleep/bad_k.sleep"), "invalid").unwrap();
    fs::write(
        project_path.join("rsb.toml"),
        "[processors]\nenabled = [\"sleep\"]\n"
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
fn test_status_command() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create a sleep file
    fs::create_dir_all(project_path.join("sleep")).unwrap();
    fs::write(project_path.join("sleep/status_test.sleep"), "0.01").unwrap();
    fs::write(
        project_path.join("rsb.toml"),
        "[processors]\nenabled = [\"sleep\"]\n"
    ).unwrap();

    // Before building, should be STALE
    let status1 = run_rsb_with_env(project_path, &["status"], &[("NO_COLOR", "1")]);
    assert!(status1.status.success());
    let stdout1 = String::from_utf8_lossy(&status1.stdout);
    assert!(stdout1.contains("STALE"), "Before build, product should be STALE: {}", stdout1);

    // Build it
    let build = run_rsb(project_path, &["build"]);
    assert!(build.status.success());

    // After building, should be UP-TO-DATE
    let status2 = run_rsb_with_env(project_path, &["status"], &[("NO_COLOR", "1")]);
    assert!(status2.status.success());
    let stdout2 = String::from_utf8_lossy(&status2.stdout);
    assert!(stdout2.contains("UP-TO-DATE"), "After build, product should be UP-TO-DATE: {}", stdout2);

    // Delete output, should be RESTORABLE (cache still exists)
    fs::remove_file(project_path.join("out/sleep/status_test.done")).unwrap();
    let status3 = run_rsb_with_env(project_path, &["status"], &[("NO_COLOR", "1")]);
    assert!(status3.status.success());
    let stdout3 = String::from_utf8_lossy(&status3.stdout);
    assert!(stdout3.contains("RESTORABLE"), "After deleting output, product should be RESTORABLE: {}", stdout3);

    // Check summary line
    assert!(stdout3.contains("Summary"), "Status output should contain Summary line");
}

#[test]
fn test_status_empty_project() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // No sleep dir, no templates to process — disable all processors
    fs::write(
        project_path.join("rsb.toml"),
        "[processors]\nenabled = []\n"
    ).unwrap();

    let output = run_rsb(project_path, &["status"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("No products discovered"), "Empty project should show 'No products discovered': {}", stdout);
}

#[test]
fn test_init_creates_project() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    let output = run_rsb(project_path, &["init"]);
    assert!(output.status.success(), "rsb init failed: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Check files/dirs were created
    assert!(project_path.join("rsb.toml").exists(), "rsb.toml should be created");
    assert!(project_path.join("templates").exists(), "templates/ should be created");
    assert!(project_path.join("config").exists(), "config/ should be created");

    // Verify rsb.toml has content
    let toml_content = fs::read_to_string(project_path.join("rsb.toml")).unwrap();
    assert!(toml_content.contains("[build]"), "rsb.toml should contain [build] section");
    assert!(toml_content.contains("[processors]"), "rsb.toml should contain [processors] section");

    assert!(stdout.contains("Created"), "Output should mention Created");
}

#[test]
fn test_init_fails_if_exists() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    // Create rsb.toml first
    fs::write(project_path.join("rsb.toml"), "# existing").unwrap();

    let output = run_rsb(project_path, &["init"]);
    assert!(!output.status.success(), "rsb init should fail if rsb.toml exists");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("already exists"), "Error should mention 'already exists': {}", stderr);
}

#[test]
fn test_init_preserves_existing_dirs() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    // Create templates dir with a file
    fs::create_dir_all(project_path.join("templates")).unwrap();
    fs::write(project_path.join("templates/existing.txt"), "do not delete").unwrap();

    let output = run_rsb(project_path, &["init"]);
    assert!(output.status.success());

    // Existing file should still be there
    assert!(project_path.join("templates/existing.txt").exists(),
        "Existing files in templates/ should be preserved");
    let content = fs::read_to_string(project_path.join("templates/existing.txt")).unwrap();
    assert_eq!(content, "do not delete");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("already exists"), "Should mention that directory already exists");
}

// ========== Dry-run tests ==========

#[test]
fn test_dry_run_shows_build_actions() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create a sleep file
    fs::create_dir_all(project_path.join("sleep")).unwrap();
    fs::write(project_path.join("sleep/dry.sleep"), "0.01").unwrap();
    fs::write(
        project_path.join("rsb.toml"),
        "[processors]\nenabled = [\"sleep\"]\n"
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
fn test_dry_run_short_flag() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    fs::create_dir_all(project_path.join("sleep")).unwrap();
    fs::write(project_path.join("sleep/short.sleep"), "0.01").unwrap();
    fs::write(
        project_path.join("rsb.toml"),
        "[processors]\nenabled = [\"sleep\"]\n"
    ).unwrap();

    let output = run_rsb_with_env(project_path, &["build", "-n"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(), "Short flag -n should work: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("BUILD"), "-n flag should show BUILD: {}", stdout);
    assert!(!project_path.join("out/sleep/short.done").exists(), "-n should not build");
}

#[test]
fn test_dry_run_with_force() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    fs::create_dir_all(project_path.join("sleep")).unwrap();
    fs::write(project_path.join("sleep/force_dry.sleep"), "0.01").unwrap();
    fs::write(
        project_path.join("rsb.toml"),
        "[processors]\nenabled = [\"sleep\"]\n"
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

// ========== Cache list tests ==========

#[test]
fn test_cache_list_shows_entries() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    fs::create_dir_all(project_path.join("sleep")).unwrap();
    fs::write(project_path.join("sleep/list_test.sleep"), "0.01").unwrap();
    fs::write(
        project_path.join("rsb.toml"),
        "[processors]\nenabled = [\"sleep\"]\n"
    ).unwrap();

    // Build to populate cache
    let build = run_rsb(project_path, &["build"]);
    assert!(build.status.success());

    // List cache
    let output = run_rsb_with_env(project_path, &["cache", "list"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("sleep"), "Cache list should contain processor name: {}", stdout);
    assert!(stdout.contains("ok"), "Cache list should show 'ok' for existing objects: {}", stdout);
    assert!(stdout.contains("cache entries"), "Cache list should show entry count: {}", stdout);
}

#[test]
fn test_cache_list_empty() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsb.toml"),
        "[processors]\nenabled = []\n"
    ).unwrap();

    let output = run_rsb_with_env(project_path, &["cache", "list"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("No cache entries"), "Empty cache should show 'No cache entries': {}", stdout);
}

// ========== Watch mode tests ==========

#[test]
fn test_watch_does_initial_build() {
    use std::process::Command;
    use std::time::Duration;

    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    fs::create_dir_all(project_path.join("sleep")).unwrap();
    fs::write(project_path.join("sleep/watch_init.sleep"), "0.01").unwrap();
    fs::write(
        project_path.join("rsb.toml"),
        "[processors]\nenabled = [\"sleep\"]\n"
    ).unwrap();

    let rsb_path = env!("CARGO_BIN_EXE_rsb");
    let mut child = Command::new(rsb_path)
        .current_dir(project_path)
        .args(["watch"])
        .env("NO_COLOR", "1")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("Failed to spawn rsb watch");

    // Wait for initial build to complete
    std::thread::sleep(Duration::from_secs(2));

    // Kill the watcher
    child.kill().expect("Failed to kill watcher");
    let output = child.wait_with_output().expect("Failed to wait on child");

    // Verify the output file was created by the initial build
    assert!(project_path.join("out/sleep/watch_init.done").exists(),
        "Watch should perform initial build");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("initial build") || stdout.contains("Processing"),
        "Watch output should mention initial build: {}", stdout);
}

#[test]
fn test_watch_rebuilds_on_change() {
    use std::process::Command;
    use std::time::Duration;

    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    fs::create_dir_all(project_path.join("sleep")).unwrap();
    fs::write(project_path.join("sleep/watch_change.sleep"), "0.01").unwrap();
    fs::write(
        project_path.join("rsb.toml"),
        "[processors]\nenabled = [\"sleep\"]\n"
    ).unwrap();

    let rsb_path = env!("CARGO_BIN_EXE_rsb");
    let mut child = Command::new(rsb_path)
        .current_dir(project_path)
        .args(["watch"])
        .env("NO_COLOR", "1")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("Failed to spawn rsb watch");

    // Wait for initial build
    std::thread::sleep(Duration::from_secs(2));

    // Modify the sleep file to trigger rebuild
    fs::write(project_path.join("sleep/watch_change.sleep"), "0.02").unwrap();

    // Wait for rebuild
    std::thread::sleep(Duration::from_secs(2));

    // Kill the watcher
    child.kill().expect("Failed to kill watcher");
    let output = child.wait_with_output().expect("Failed to wait on child");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Change detected"),
        "Watch should detect and report changes: {}", stdout);
}

// ========== C/C++ compiler processor tests ==========

/// Helper to set up a C project with the cc processor enabled
fn setup_cc_project(project_path: &Path) {
    fs::create_dir_all(project_path.join("src")).unwrap();
    fs::write(
        project_path.join("rsb.toml"),
        "[processors]\nenabled = [\"cc\"]\n"
    ).unwrap();
}

#[test]
fn test_cc_compile_single_c_file() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    setup_cc_project(project_path);

    fs::write(
        project_path.join("src/main.c"),
        "int main() { return 0; }\n"
    ).unwrap();

    let output = run_rsb_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(),
        "rsb build failed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr));

    // Check executable exists
    assert!(project_path.join("out/cc/main.elf").exists(), "Executable should exist");
}

#[test]
fn test_cc_incremental_skip() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    setup_cc_project(project_path);

    fs::write(
        project_path.join("src/main.c"),
        "int main() { return 0; }\n"
    ).unwrap();

    // First build
    let output1 = run_rsb_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output1.status.success());
    let stdout1 = String::from_utf8_lossy(&output1.stdout);
    assert!(stdout1.contains("[cc] Processing:"), "First build should process: {}", stdout1);

    // Second build - should skip
    let output2 = run_rsb_with_env(project_path, &["build", "--verbose"], &[("NO_COLOR", "1")]);
    assert!(output2.status.success());
    let stdout2 = String::from_utf8_lossy(&output2.stdout);
    assert!(stdout2.contains("[cc] Skipping (unchanged):"), "Second build should skip: {}", stdout2);
}

#[test]
fn test_cc_header_dependency() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    setup_cc_project(project_path);

    // Create header and source
    fs::write(
        project_path.join("src/utils.h"),
        "#ifndef UTILS_H\n#define UTILS_H\n#define VALUE 42\n#endif\n"
    ).unwrap();

    fs::write(
        project_path.join("src/main.c"),
        "#include \"utils.h\"\nint main() { return VALUE - 42; }\n"
    ).unwrap();

    // First build
    let output1 = run_rsb_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output1.status.success(),
        "First build failed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output1.stdout),
        String::from_utf8_lossy(&output1.stderr));

    // Wait a moment so mtime differs
    std::thread::sleep(std::time::Duration::from_millis(100));

    // Modify header (keep VALUE defined so compilation still succeeds)
    fs::write(
        project_path.join("src/utils.h"),
        "#ifndef UTILS_H\n#define UTILS_H\n#define VALUE 42\n#define OTHER 10\n#endif\n"
    ).unwrap();

    // Rebuild - should recompile files that include utils.h
    let output2 = run_rsb_with_env(project_path, &["build", "--verbose"], &[("NO_COLOR", "1")]);
    assert!(output2.status.success(),
        "Rebuild failed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output2.stdout),
        String::from_utf8_lossy(&output2.stderr));
    let stdout2 = String::from_utf8_lossy(&output2.stdout);
    assert!(stdout2.contains("[cc] Processing:"),
        "Should recompile after header change: {}", stdout2);
}

#[test]
fn test_cc_mixed_c_and_cpp() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    setup_cc_project(project_path);

    fs::write(
        project_path.join("src/helper.c"),
        "int main() { return 0; }\n"
    ).unwrap();

    fs::write(
        project_path.join("src/main.cc"),
        "int main() { return 0; }\n"
    ).unwrap();

    let output = run_rsb_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(),
        "Mixed C/C++ build failed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr));

    assert!(project_path.join("out/cc/helper.elf").exists(), "C executable should exist");
    assert!(project_path.join("out/cc/main.elf").exists(), "C++ executable should exist");
}

#[test]
fn test_cc_clean() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    setup_cc_project(project_path);

    fs::write(
        project_path.join("src/main.c"),
        "int main() { return 0; }\n"
    ).unwrap();

    // Build
    let build_output = run_rsb(project_path, &["build"]);
    assert!(build_output.status.success());
    assert!(project_path.join("out/cc/main.elf").exists());

    // Clean
    let clean_output = run_rsb(project_path, &["clean"]);
    assert!(clean_output.status.success());

    // Verify outputs are removed but cache is preserved
    assert!(!project_path.join("out/cc").exists(), "out/cc/ should be removed after clean");
    assert!(project_path.join(".rsb/deps").exists(), "deps cache should be preserved after clean");
}

#[test]
fn test_cc_dry_run() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    setup_cc_project(project_path);

    fs::write(
        project_path.join("src/main.c"),
        "int main() { return 0; }\n"
    ).unwrap();

    // Dry run
    let output = run_rsb_with_env(project_path, &["build", "--dry-run"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("BUILD"), "Dry run should show BUILD for cc products: {}", stdout);

    // Verify nothing was built
    assert!(!project_path.join("out/cc/main.elf").exists(), "Dry run should not compile");
}

// ========== .rsbignore tests ==========

#[test]
fn test_rsbignore_excludes_sleep_files() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create sleep directory with two files
    fs::create_dir_all(project_path.join("sleep")).unwrap();
    fs::write(project_path.join("sleep/included.sleep"), "0.01").unwrap();
    fs::write(project_path.join("sleep/excluded.sleep"), "0.01").unwrap();

    fs::write(
        project_path.join("rsb.toml"),
        "[processors]\nenabled = [\"sleep\"]\n"
    ).unwrap();

    // Create .rsbignore that excludes one file
    fs::write(
        project_path.join(".rsbignore"),
        "sleep/excluded.sleep\n"
    ).unwrap();

    // Build
    let output = run_rsb_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(), "rsb build failed: {}", String::from_utf8_lossy(&output.stderr));

    // The included file should be processed
    assert!(project_path.join("out/sleep/included.done").exists(),
        "Included sleep file should be processed");

    // The excluded file should NOT be processed
    assert!(!project_path.join("out/sleep/excluded.done").exists(),
        "Excluded sleep file should not be processed");
}

#[test]
fn test_rsbignore_glob_pattern() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create sleep directory with subdirectory
    fs::create_dir_all(project_path.join("sleep/subdir")).unwrap();
    fs::write(project_path.join("sleep/keep.sleep"), "0.01").unwrap();
    fs::write(project_path.join("sleep/subdir/skip1.sleep"), "0.01").unwrap();
    fs::write(project_path.join("sleep/subdir/skip2.sleep"), "0.01").unwrap();

    fs::write(
        project_path.join("rsb.toml"),
        "[processors]\nenabled = [\"sleep\"]\n"
    ).unwrap();

    // Use a glob pattern to exclude the entire subdirectory
    fs::write(
        project_path.join(".rsbignore"),
        "# Exclude all files in subdir\nsleep/subdir/**\n"
    ).unwrap();

    // Build
    let output = run_rsb_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(), "rsb build failed: {}", String::from_utf8_lossy(&output.stderr));

    // keep.sleep should be processed
    assert!(project_path.join("out/sleep/keep.done").exists(),
        "keep.sleep should be processed");

    // subdir files should be excluded
    assert!(!project_path.join("out/sleep/skip1.done").exists(),
        "subdir/skip1.sleep should be excluded");
    assert!(!project_path.join("out/sleep/skip2.done").exists(),
        "subdir/skip2.sleep should be excluded");
}

#[test]
fn test_rsbignore_no_file() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create sleep directory — no .rsbignore
    fs::create_dir_all(project_path.join("sleep")).unwrap();
    fs::write(project_path.join("sleep/normal.sleep"), "0.01").unwrap();

    fs::write(
        project_path.join("rsb.toml"),
        "[processors]\nenabled = [\"sleep\"]\n"
    ).unwrap();

    // Build should work fine without .rsbignore
    let output = run_rsb_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(), "rsb build failed without .rsbignore: {}",
        String::from_utf8_lossy(&output.stderr));

    assert!(project_path.join("out/sleep/normal.done").exists(),
        "Sleep file should be processed when no .rsbignore exists");
}

#[test]
fn test_rsbignore_comments_and_blank_lines() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    fs::create_dir_all(project_path.join("sleep")).unwrap();
    fs::write(project_path.join("sleep/a.sleep"), "0.01").unwrap();
    fs::write(project_path.join("sleep/b.sleep"), "0.01").unwrap();

    fs::write(
        project_path.join("rsb.toml"),
        "[processors]\nenabled = [\"sleep\"]\n"
    ).unwrap();

    // .rsbignore with comments, blank lines, and one real pattern
    fs::write(
        project_path.join(".rsbignore"),
        "# This is a comment\n\n   \n# Another comment\nsleep/b.sleep\n\n"
    ).unwrap();

    let output = run_rsb_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());

    assert!(project_path.join("out/sleep/a.done").exists(), "a.sleep should be processed");
    assert!(!project_path.join("out/sleep/b.done").exists(), "b.sleep should be ignored");
}

#[test]
fn test_rsbignore_cc_processor() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    setup_cc_project(project_path);

    // Create two C files
    fs::write(
        project_path.join("src/included.c"),
        "int main() { return 0; }\n"
    ).unwrap();

    fs::create_dir_all(project_path.join("src/excluded")).unwrap();
    fs::write(
        project_path.join("src/excluded/skip.c"),
        "int main() { return 0; }\n"
    ).unwrap();

    // Exclude the subdirectory
    fs::write(
        project_path.join(".rsbignore"),
        "src/excluded/**\n"
    ).unwrap();

    let output = run_rsb_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(),
        "Build failed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr));

    assert!(project_path.join("out/cc/included.elf").exists(),
        "included.c should be compiled");
    assert!(!project_path.join("out/cc/excluded/skip.elf").exists(),
        "excluded/skip.c should not be compiled");
}

#[test]
fn test_rsbignore_leading_slash() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    setup_cc_project(project_path);

    fs::write(
        project_path.join("src/keep.c"),
        "int main() { return 0; }\n"
    ).unwrap();

    fs::create_dir_all(project_path.join("src/skip_dir")).unwrap();
    fs::write(
        project_path.join("src/skip_dir/skip.c"),
        "int main() { return 0; }\n"
    ).unwrap();

    // Leading '/' should work like .gitignore (anchored to project root)
    fs::write(
        project_path.join(".rsbignore"),
        "/src/skip_dir/**\n"
    ).unwrap();

    let output = run_rsb_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(),
        "Build failed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr));

    assert!(project_path.join("out/cc/keep.elf").exists(),
        "keep.c should be compiled");
    assert!(!project_path.join("out/cc/skip_dir/skip.elf").exists(),
        "skip_dir/skip.c should be excluded by /src/skip_dir/** pattern");
}

#[test]
fn test_rsbignore_trailing_slash() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    setup_cc_project(project_path);

    fs::write(
        project_path.join("src/keep.c"),
        "int main() { return 0; }\n"
    ).unwrap();

    fs::create_dir_all(project_path.join("src/skipme")).unwrap();
    fs::write(
        project_path.join("src/skipme/deep.c"),
        "int main() { return 0; }\n"
    ).unwrap();

    // Trailing '/' should exclude the directory and all its contents
    fs::write(
        project_path.join(".rsbignore"),
        "/src/skipme/\n"
    ).unwrap();

    let output = run_rsb_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(),
        "Build failed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr));

    assert!(project_path.join("out/cc/keep.elf").exists(),
        "keep.c should be compiled");
    assert!(!project_path.join("out/cc/skipme/deep.elf").exists(),
        "skipme/deep.c should be excluded by /src/skipme/ pattern");
}

// ========== Deterministic build order tests ==========

#[test]
fn test_deterministic_build_order() {
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
            "[processors]\nenabled = [\"sleep\"]\n"
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

// ========== Per-file compile/link flags tests ==========

#[test]
fn test_cc_per_file_compile_flags() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    setup_cc_project(project_path);

    // Source with EXTRA_COMPILE_FLAGS_AFTER defining a macro
    fs::write(
        project_path.join("src/flagtest.c"),
        r#"// EXTRA_COMPILE_FLAGS_AFTER=-DTEST_VALUE=42
#include <stdio.h>
int main() {
    printf("%d\n", TEST_VALUE);
    return 0;
}
"#
    ).unwrap();

    let output = run_rsb_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(),
        "Build with per-file compile flags failed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr));

    assert!(project_path.join("out/cc/flagtest.elf").exists(),
        "Executable with per-file compile flags should exist");

    // Run the executable and verify it outputs 42
    let run_output = Command::new(project_path.join("out/cc/flagtest.elf"))
        .output()
        .expect("Failed to run flagtest");
    let stdout = String::from_utf8_lossy(&run_output.stdout);
    assert!(stdout.trim() == "42",
        "Executable should output 42, got: {}", stdout.trim());
}

#[test]
fn test_cc_per_file_link_flags() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    setup_cc_project(project_path);

    // Source that uses math library (sqrt), linked via per-file flag
    fs::write(
        project_path.join("src/mathtest.c"),
        r#"// EXTRA_LINK_FLAGS_AFTER=-lm
#include <stdio.h>
#include <math.h>
int main() {
    printf("%.0f\n", sqrt(144.0));
    return 0;
}
"#
    ).unwrap();

    let output = run_rsb_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(),
        "Build with per-file link flags failed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr));

    assert!(project_path.join("out/cc/mathtest.elf").exists(),
        "Executable with per-file link flags should exist");

    // Run the executable and verify it outputs 12
    let run_output = Command::new(project_path.join("out/cc/mathtest.elf"))
        .output()
        .expect("Failed to run mathtest");
    let stdout = String::from_utf8_lossy(&run_output.stdout);
    assert!(stdout.trim() == "12",
        "Executable should output 12, got: {}", stdout.trim());
}

#[test]
fn test_cc_per_file_backtick_substitution() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    setup_cc_project(project_path);

    // Source with backtick command substitution to define a macro
    fs::write(
        project_path.join("src/backtick.c"),
        r#"// EXTRA_COMPILE_FLAGS_AFTER=`echo -DBACKTICK_VAL=99`
#include <stdio.h>
int main() {
    printf("%d\n", BACKTICK_VAL);
    return 0;
}
"#
    ).unwrap();

    let output = run_rsb_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(),
        "Build with backtick substitution failed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr));

    assert!(project_path.join("out/cc/backtick.elf").exists(),
        "Executable with backtick substitution should exist");

    // Run the executable and verify it outputs 99
    let run_output = Command::new(project_path.join("out/cc/backtick.elf"))
        .output()
        .expect("Failed to run backtick");
    let stdout = String::from_utf8_lossy(&run_output.stdout);
    assert!(stdout.trim() == "99",
        "Executable should output 99, got: {}", stdout.trim());
}

#[test]
fn test_cc_per_file_no_flags() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    setup_cc_project(project_path);

    // Source without any special comments
    fs::write(
        project_path.join("src/plain.c"),
        r#"#include <stdio.h>
int main() {
    printf("hello\n");
    return 0;
}
"#
    ).unwrap();

    let output = run_rsb_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(),
        "Build without per-file flags failed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr));

    assert!(project_path.join("out/cc/plain.elf").exists(),
        "Executable without per-file flags should exist");

    let run_output = Command::new(project_path.join("out/cc/plain.elf"))
        .output()
        .expect("Failed to run plain");
    let stdout = String::from_utf8_lossy(&run_output.stdout);
    assert!(stdout.trim() == "hello",
        "Executable should output hello, got: {}", stdout.trim());
}

#[test]
fn test_cc_per_file_compile_cmd() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    setup_cc_project(project_path);

    // EXTRA_COMPILE_CMD runs a command as subprocess; use echo to produce a -D flag
    fs::write(
        project_path.join("src/compilecmd.c"),
        r#"// EXTRA_COMPILE_CMD=echo -DCMD_VAL=77
#include <stdio.h>
int main() {
    printf("%d\n", CMD_VAL);
    return 0;
}
"#
    ).unwrap();

    let output = run_rsb_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(),
        "Build with EXTRA_COMPILE_CMD failed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr));

    assert!(project_path.join("out/cc/compilecmd.elf").exists(),
        "Executable with EXTRA_COMPILE_CMD should exist");

    let run_output = Command::new(project_path.join("out/cc/compilecmd.elf"))
        .output()
        .expect("Failed to run compilecmd");
    let stdout = String::from_utf8_lossy(&run_output.stdout);
    assert!(stdout.trim() == "77",
        "Executable should output 77, got: {}", stdout.trim());
}

#[test]
fn test_cc_per_file_link_cmd() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    setup_cc_project(project_path);

    // EXTRA_LINK_CMD runs a command; use echo to produce -lm
    fs::write(
        project_path.join("src/linkcmd.c"),
        r#"// EXTRA_LINK_CMD=echo -lm
#include <stdio.h>
#include <math.h>
int main() {
    printf("%.0f\n", sqrt(144.0));
    return 0;
}
"#
    ).unwrap();

    let output = run_rsb_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(),
        "Build with EXTRA_LINK_CMD failed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr));

    assert!(project_path.join("out/cc/linkcmd.elf").exists(),
        "Executable with EXTRA_LINK_CMD should exist");

    let run_output = Command::new(project_path.join("out/cc/linkcmd.elf"))
        .output()
        .expect("Failed to run linkcmd");
    let stdout = String::from_utf8_lossy(&run_output.stdout);
    assert!(stdout.trim() == "12",
        "Executable should output 12, got: {}", stdout.trim());
}

#[test]
fn test_cc_per_file_block_comment_star_prefix() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    setup_cc_project(project_path);

    // Block comment continuation line with * prefix
    fs::write(
        project_path.join("src/blockstar.c"),
        r#"/*
 * EXTRA_LINK_FLAGS_AFTER=-lm
 */
#include <stdio.h>
#include <math.h>
int main() {
    printf("%.0f\n", sqrt(144.0));
    return 0;
}
"#
    ).unwrap();

    let output = run_rsb_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(),
        "Build with block comment * prefix failed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr));

    assert!(project_path.join("out/cc/blockstar.elf").exists(),
        "Executable with block comment * prefix should exist");

    let run_output = Command::new(project_path.join("out/cc/blockstar.elf"))
        .output()
        .expect("Failed to run blockstar");
    let stdout = String::from_utf8_lossy(&run_output.stdout);
    assert!(stdout.trim() == "12",
        "Executable should output 12, got: {}", stdout.trim());
}

#[test]
fn test_cc_per_file_compile_shell() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    setup_cc_project(project_path);

    // Use EXTRA_COMPILE_SHELL to define a macro via shell command
    fs::write(
        project_path.join("src/compileshell.c"),
        r#"// EXTRA_COMPILE_SHELL=echo -DSHELL_VALUE=$(echo 77)
#include <stdio.h>
int main() {
    printf("%d\n", SHELL_VALUE);
    return 0;
}
"#
    ).unwrap();

    let output = run_rsb_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(),
        "Build with EXTRA_COMPILE_SHELL failed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr));

    assert!(project_path.join("out/cc/compileshell.elf").exists(),
        "Executable with EXTRA_COMPILE_SHELL should exist");

    let run_output = Command::new(project_path.join("out/cc/compileshell.elf"))
        .output()
        .expect("Failed to run compileshell");
    let stdout = String::from_utf8_lossy(&run_output.stdout);
    assert!(stdout.trim() == "77",
        "Executable should output 77, got: {}", stdout.trim());
}

#[test]
fn test_cc_per_file_link_shell() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    setup_cc_project(project_path);

    // Use EXTRA_LINK_SHELL to add -lm via shell
    fs::write(
        project_path.join("src/linkshell.c"),
        r#"// EXTRA_LINK_SHELL=echo -lm
#include <stdio.h>
#include <math.h>
int main() {
    printf("%.0f\n", sqrt(49.0));
    return 0;
}
"#
    ).unwrap();

    let output = run_rsb_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(),
        "Build with EXTRA_LINK_SHELL failed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr));

    assert!(project_path.join("out/cc/linkshell.elf").exists(),
        "Executable with EXTRA_LINK_SHELL should exist");

    let run_output = Command::new(project_path.join("out/cc/linkshell.elf"))
        .output()
        .expect("Failed to run linkshell");
    let stdout = String::from_utf8_lossy(&run_output.stdout);
    assert!(stdout.trim() == "7",
        "Executable should output 7, got: {}", stdout.trim());
}