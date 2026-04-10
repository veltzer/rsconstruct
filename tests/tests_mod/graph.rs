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
        project_path.join("tera.templates/out.txt.tera"),
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

#[test]
fn graph_unreferenced_finds_untracked_files() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let project_path = temp_dir.path();

    // Processor scans only the "src" subdir
    fs::create_dir(project_path.join("src")).unwrap();
    fs::write(project_path.join("rsconstruct.toml"), concat!(
        "[processor.script]\n",
        "command = \"true\"\n",
        "src_extensions = [\".txt\"]\n",
        "src_dirs = [\"src\"]\n",
    )).unwrap();

    // This file IS referenced (primary input inside src/)
    fs::write(project_path.join("src/used.txt"), "used\n").unwrap();
    // This file is NOT referenced (outside src/, not picked up by processor)
    fs::write(project_path.join("unused.txt"), "unused\n").unwrap();

    run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);

    let output = run_rsconstruct_with_env(
        project_path,
        &["graph", "unreferenced", "--extensions", ".txt"],
        &[("NO_COLOR", "1")],
    );
    assert!(output.status.success(), "graph unreferenced failed: {}", String::from_utf8_lossy(&output.stderr));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("unused.txt"), "Should report unused.txt: {}", stdout);
    assert!(!stdout.contains("src/used.txt"), "Should not report src/used.txt: {}", stdout);
}

#[test]
fn graph_unreferenced_dep_inputs_not_reported() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let project_path = temp_dir.path();

    fs::create_dir(project_path.join("src")).unwrap();

    // dep.txt is a dependency (dep_inputs) — referenced but not a primary input
    fs::write(project_path.join("dep.txt"), "dep\n").unwrap();
    // source.txt is a primary input
    fs::write(project_path.join("src/source.txt"), "source\n").unwrap();
    // unused.txt is neither
    fs::write(project_path.join("unused.txt"), "unused\n").unwrap();

    fs::write(project_path.join("rsconstruct.toml"), concat!(
        "[processor.script]\n",
        "command = \"true\"\n",
        "src_extensions = [\".txt\"]\n",
        "src_dirs = [\"src\"]\n",
        "dep_inputs = [\"dep.txt\"]\n",
    )).unwrap();

    run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);

    let output = run_rsconstruct_with_env(
        project_path,
        &["graph", "unreferenced", "--extensions", ".txt"],
        &[("NO_COLOR", "1")],
    );
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("dep.txt"), "dep.txt is referenced via dep_inputs, should not appear: {}", stdout);
    assert!(!stdout.contains("source.txt"), "source.txt is a primary input, should not appear: {}", stdout);
    assert!(stdout.contains("unused.txt"), "unused.txt should appear: {}", stdout);
}

#[test]
fn graph_unreferenced_rm_deletes_files() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let project_path = temp_dir.path();

    fs::create_dir(project_path.join("src")).unwrap();
    fs::write(project_path.join("rsconstruct.toml"), concat!(
        "[processor.script]\n",
        "command = \"true\"\n",
        "src_extensions = [\".txt\"]\n",
        "src_dirs = [\"src\"]\n",
    )).unwrap();

    fs::write(project_path.join("src/used.txt"), "used\n").unwrap();
    fs::write(project_path.join("unused.txt"), "unused\n").unwrap();

    run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);

    let output = run_rsconstruct_with_env(
        project_path,
        &["graph", "unreferenced", "--extensions", ".txt", "--rm"],
        &[("NO_COLOR", "1")],
    );
    assert!(output.status.success());

    assert!(!project_path.join("unused.txt").exists(), "unused.txt should have been deleted");
    assert!(project_path.join("src/used.txt").exists(), "src/used.txt should still exist");
}
