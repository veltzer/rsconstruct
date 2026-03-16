use std::fs;
use crate::common::{setup_test_project, run_rsconstruct_with_env};

fn setup_project_with_template() -> tempfile::TempDir {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("config/test.py"),
        "value = 1"
    ).expect("Failed to write config");
    fs::write(
        project_path.join("templates.tera/out.txt.tera"),
        "{% set c = load_python(path='config/test.py') %}{{ c.value }}"
    ).expect("Failed to write template");

    temp_dir
}

#[test]
fn graph_dot_format() {
    let temp_dir = setup_project_with_template();
    let project_path = temp_dir.path();

    let output = run_rsconstruct_with_env(project_path, &["graph", "show", "--format", "dot"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(), "graph --format dot failed: {}", String::from_utf8_lossy(&output.stderr));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("digraph"), "Expected DOT digraph in output");
}

#[test]
fn graph_mermaid_format() {
    let temp_dir = setup_project_with_template();
    let project_path = temp_dir.path();

    let output = run_rsconstruct_with_env(project_path, &["graph", "show", "--format", "mermaid"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(), "graph --format mermaid failed: {}", String::from_utf8_lossy(&output.stderr));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("graph") || stdout.contains("flowchart"),
        "Expected mermaid graph syntax in output, got: {}", stdout);
}

#[test]
fn graph_json_format() {
    let temp_dir = setup_project_with_template();
    let project_path = temp_dir.path();

    let output = run_rsconstruct_with_env(project_path, &["graph", "show", "--format", "json"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(), "graph --format json failed: {}", String::from_utf8_lossy(&output.stderr));

    let stdout = String::from_utf8_lossy(&output.stdout);
    // JSON output should be valid JSON with array or object structure
    assert!(stdout.starts_with('{') || stdout.starts_with('['),
        "Expected JSON output, got: {}", stdout);
}

#[test]
fn graph_text_format() {
    let temp_dir = setup_project_with_template();
    let project_path = temp_dir.path();

    let output = run_rsconstruct_with_env(project_path, &["graph", "show", "--format", "text"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(), "graph --format text failed: {}", String::from_utf8_lossy(&output.stderr));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.is_empty(), "Expected non-empty text graph output");
}

#[test]
fn graph_empty_project() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // No template files, so the graph should be empty but command should succeed
    let output = run_rsconstruct_with_env(project_path, &["graph", "show", "--format", "json"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(), "graph on empty project failed: {}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn graph_stats() {
    let temp_dir = setup_project_with_template();
    let project_path = temp_dir.path();

    let output = run_rsconstruct_with_env(project_path, &["graph", "stats"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(), "graph stats failed: {}", String::from_utf8_lossy(&output.stderr));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("products"), "Expected 'products' in stats output, got: {}", stdout);
}

#[test]
fn graph_stats_json() {
    let temp_dir = setup_project_with_template();
    let project_path = temp_dir.path();

    let output = run_rsconstruct_with_env(project_path, &["--json", "graph", "stats"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(), "graph stats --json failed: {}", String::from_utf8_lossy(&output.stderr));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.starts_with('{'), "Expected JSON output, got: {}", stdout);
}
