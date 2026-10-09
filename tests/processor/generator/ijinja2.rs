use crate::common::run_rsconstruct_with_env;
use std::fs;
use tempfile::TempDir;

/// A project with the ijinja2 generator over `templates.jinja2/`.
fn setup_project() -> TempDir {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    fs::create_dir_all(temp_dir.path().join("templates.jinja2"))
        .expect("Failed to create templates.jinja2 dir");
    fs::write(
        temp_dir.path().join("rsconstruct.toml"),
        "[processor.generator.ijinja2]\nsrc_dirs = [\"templates.jinja2\"]\n",
    )
    .expect("Failed to write rsconstruct.toml");
    temp_dir
}

fn build(project: &std::path::Path, env: &[(&str, &str)]) -> std::process::Output {
    let mut vars = vec![("NO_COLOR", "1")];
    vars.extend_from_slice(env);
    run_rsconstruct_with_env(project, &["build"], &vars)
}

#[test]
fn ijinja2_basic_render() {
    let temp_dir = setup_project();
    let project = temp_dir.path();
    fs::write(
        project.join("templates.jinja2/hello.txt.j2"),
        "Hello, {{ 'World' }}!\nCount: {{ 2 + 3 }}\n",
    )
    .unwrap();
    let output = build(project, &[]);
    assert!(
        output.status.success(),
        "rsconstruct build failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let content = fs::read_to_string(project.join("hello.txt")).expect("output written");
    // Jinja2 drops the single trailing newline of a template.
    assert_eq!(content, "Hello, World!\nCount: 5");
}

#[test]
fn ijinja2_environment_is_the_context_and_includes_resolve_from_the_root() {
    let temp_dir = setup_project();
    let project = temp_dir.path();
    fs::create_dir_all(project.join("templates.jinja2/sub")).unwrap();
    fs::write(
        project.join("templates.jinja2/header.inc"),
        "== {{ TITLE | upper }} ==\n",
    )
    .unwrap();
    fs::write(
        project.join("templates.jinja2/sub/page.md.j2"),
        "{% include 'templates.jinja2/header.inc' %}{% for i in range(3) %}{{ i }}{% if not loop.last %}, {% endif %}{% endfor %}\n{{ MISSING }}|{{ MISSING | default('none') }}\n",
    )
    .unwrap();
    let output = build(project, &[("TITLE", "docs")]);
    assert!(
        output.status.success(),
        "rsconstruct build failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let content = fs::read_to_string(project.join("sub/page.md")).expect("output written");
    // The included file's own trailing newline is dropped too, as in Jinja2.
    assert_eq!(content, "== DOCS ==0, 1, 2\n|none");
}

/// The filters whose Jinja2 argument lists ijinja2 supplies itself; the
/// expected text is Python Jinja2's output for the same template.
#[test]
fn ijinja2_jinja2_filter_arguments() {
    let temp_dir = setup_project();
    let project = temp_dir.path();
    fs::write(
        project.join("templates.jinja2/f.txt.j2"),
        "{{ 'hello world' | truncate(8, true) }}|{{ 'the quick brown fox' | truncate(11) }}|{{ 'aaa' | replace('a','b',2) }}|{{ 3.5 | round(0, 'floor') }}|{{ 2.5 | round(0, 'ceil') }}|{{ 3.14159 | round(2) }}|{{ '0x1f' | int(0, 16) }}|{{ 'z' | int(7) }}|{{ [1,2] | sum(start=10) }}|{{ [{'v': 1}, {'v': 2.5}] | sum(attribute='v') }}|{{ 'ab' | center(6) }}|{{ 'abc' | center(6) }}|{{ '<a>' | forceescape }}",
    )
    .unwrap();
    let output = build(project, &[]);
    assert!(
        output.status.success(),
        "rsconstruct build failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let content = fs::read_to_string(project.join("f.txt")).expect("output written");
    assert_eq!(
        content,
        "hello world|the...|bba|3.0|3.0|3.14|31|7|13|3.5|  ab  | abc  |&lt;a&gt;"
    );
}

#[test]
fn ijinja2_reports_template_errors() {
    let temp_dir = setup_project();
    let project = temp_dir.path();
    fs::write(
        project.join("templates.jinja2/bad.txt.j2"),
        "{% if x %}unclosed\n",
    )
    .unwrap();
    let output = build(project, &[]);
    assert!(
        !output.status.success(),
        "a syntax error must fail the build"
    );
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        text.contains("templates.jinja2/bad.txt.j2"),
        "the failing template must be named: {text}"
    );
    assert!(
        !project.join("bad.txt").exists(),
        "no output for a failed render"
    );
}
