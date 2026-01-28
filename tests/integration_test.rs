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

    // Verify files are removed
    assert!(!project_path.join("cleanme.txt").exists());
    assert!(!project_path.join(".rsb").exists());
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