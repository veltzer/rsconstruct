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
    let output = run_rsb(project_path, &["build"]);
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
    let output1 = run_rsb(project_path, &["build"]);
    assert!(output1.status.success());
    let stdout1 = String::from_utf8_lossy(&output1.stdout);
    assert!(stdout1.contains("[template] Processing:"));

    // Second build (should skip unchanged template - use verbose to see skip message)
    let output2 = run_rsb(project_path, &["build", "--verbose"]);
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
    let output = run_rsb(project_path, &["build", "--force"]);
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