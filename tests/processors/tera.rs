use std::fs;
use crate::common::{setup_test_project, run_rsb, run_rsb_with_env};

#[test]
fn tera_to_file_translation() {
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

    // Create a tera file
    let tera_content = r#"{% set cfg = load_python(path="config/test_config.py") %}
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
        tera_content
    ).expect("Failed to write tera file");

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
fn incremental_build() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create a simple config and tera
    fs::write(
        project_path.join("config/simple.py"),
        "name = 'SimpleTest'\ncount = 42"
    ).expect("Failed to write config");

    fs::write(
        project_path.join("templates/simple.txt.tera"),
        "{% set c = load_python(path='config/simple.py') %}Name: {{ c.name }}, Count: {{ c.count }}"
    ).expect("Failed to write tera");

    // First build
    let output1 = run_rsb_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output1.status.success());
    let stdout1 = String::from_utf8_lossy(&output1.stdout);
    assert!(stdout1.contains("Processing:"));

    // Second build (should skip unchanged tera - use verbose to see skip message)
    let output2 = run_rsb_with_env(project_path, &["build", "--verbose"], &[("NO_COLOR", "1")]);
    assert!(output2.status.success());
    let stdout2 = String::from_utf8_lossy(&output2.stdout);
    assert!(stdout2.contains("[tera] Skipping (unchanged):"));

    // Verify cache directory exists
    assert!(project_path.join(".rsb/db").exists());
}

#[test]
fn multiple_templates() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create shared config
    let config = "shared_name = 'MultiTest'\nshared_value = 123";
    fs::write(project_path.join("config/shared.py"), config).unwrap();

    // Create multiple teras
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
fn extra_inputs_triggers_rebuild() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create a Python config file used as extra_input
    fs::write(
        project_path.join("config/settings.py"),
        "name = 'Original'"
    ).unwrap();

    // Create a tera
    fs::write(
        project_path.join("templates/output.txt.tera"),
        "{% set c = load_python(path='config/settings.py') %}Name: {{ c.name }}"
    ).unwrap();

    // Configure tera processor with extra_inputs pointing to the config file
    fs::write(
        project_path.join("rsb.toml"),
        "[processor]\nenabled = [\"tera\"]\n\n[processor.tera]\nextra_inputs = [\"config/settings.py\"]\n"
    ).unwrap();

    // First build
    let output1 = run_rsb_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output1.status.success(),
        "First build failed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output1.stdout),
        String::from_utf8_lossy(&output1.stderr));
    let stdout1 = String::from_utf8_lossy(&output1.stdout);
    assert!(stdout1.contains("Processing:"), "First build should process: {}", stdout1);

    // Second build — should skip (nothing changed)
    let output2 = run_rsb_with_env(project_path, &["build", "--verbose"], &[("NO_COLOR", "1")]);
    assert!(output2.status.success());
    let stdout2 = String::from_utf8_lossy(&output2.stdout);
    assert!(stdout2.contains("[tera] Skipping (unchanged):"), "Second build should skip: {}", stdout2);

    // Wait so mtime differs
    std::thread::sleep(std::time::Duration::from_millis(100));

    // Modify the extra input file (but not the tera itself)
    fs::write(
        project_path.join("config/settings.py"),
        "name = 'Modified'"
    ).unwrap();

    // Third build — should rebuild because extra input changed
    let output3 = run_rsb_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output3.status.success(),
        "Third build failed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output3.stdout),
        String::from_utf8_lossy(&output3.stderr));
    let stdout3 = String::from_utf8_lossy(&output3.stdout);
    assert!(stdout3.contains("Processing:"),
        "Build after extra_input change should reprocess, not skip: {}", stdout3);

    // Verify the output reflects the new config
    let content = fs::read_to_string(project_path.join("output.txt")).unwrap();
    assert!(content.contains("Modified"),
        "Output should reflect the modified config: {}", content);
}

#[test]
fn extra_inputs_nonexistent_file_fails() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create a tera
    fs::write(
        project_path.join("config/simple.py"),
        "val = 'test'"
    ).unwrap();

    fs::write(
        project_path.join("templates/simple.txt.tera"),
        "{% set c = load_python(path='config/simple.py') %}{{ c.val }}"
    ).unwrap();

    // Configure with a nonexistent extra_input — should cause an error
    fs::write(
        project_path.join("rsb.toml"),
        "[processor]\nenabled = [\"tera\"]\n\n[processor.tera]\nextra_inputs = [\"nonexistent_file.txt\"]\n"
    ).unwrap();

    let output = run_rsb_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(!output.status.success(),
        "Build should fail with nonexistent extra_input: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr));

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("extra_inputs file not found") || stderr.contains("nonexistent_file.txt"),
        "Error should mention missing extra_inputs file: {}", stderr);
}
