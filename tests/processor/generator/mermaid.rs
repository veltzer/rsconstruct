use crate::common::{require_tool, run_rsconstruct_with_env};
use std::fs;
use tempfile::TempDir;

#[test]
fn mermaid_discovery() {
    require_tool("mmdc");

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.generator.mermaid]\nsrc_dirs = [\".\"]\n",
    )
    .unwrap();

    fs::write(
        project_path.join("diagram.mmd"),
        "graph TD\n    A --> B\n    B --> C\n",
    )
    .unwrap();

    let output =
        run_rsconstruct_with_env(project_path, &["build", "--dry-run"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "Dry run should succeed with valid Mermaid file: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("BUILD") || stdout.contains("build"),
        "Should discover mermaid product: {}",
        stdout
    );
}

/// A real render: mmdc drives a headless Chromium through Puppeteer.
/// Chromium's sandbox needs unprivileged user namespaces, which Ubuntu
/// 23.10+ restricts through AppArmor, so the project hands mmdc a Puppeteer
/// config that launches it with `--no-sandbox` (the rendered input is the
/// test's own diagram).
#[test]
fn mermaid_builds_png_and_svg() {
    require_tool("mmdc");

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.generator.mermaid]\nsrc_dirs = [\".\"]\nformats = [\"png\", \"svg\"]\nargs = [\"-p\", \"puppeteer.json\"]\n",
    )
    .unwrap();
    fs::write(
        project_path.join("puppeteer.json"),
        "{\"args\": [\"--no-sandbox\"]}\n",
    )
    .unwrap();
    fs::write(
        project_path.join("diagram.mmd"),
        "graph TD\n    A --> B\n    B --> C\n",
    )
    .unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "Build should render the Mermaid file: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let png = project_path.join("out/processor.generator.mermaid/diagram.png");
    let bytes = fs::read(&png).unwrap_or_else(|e| panic!("{} not produced: {e}", png.display()));
    assert!(
        bytes.starts_with(b"\x89PNG"),
        "{} is not a PNG",
        png.display()
    );
    let svg = project_path.join("out/processor.generator.mermaid/diagram.svg");
    let text =
        fs::read_to_string(&svg).unwrap_or_else(|e| panic!("{} not produced: {e}", svg.display()));
    assert!(
        text.contains("<svg"),
        "{} is not an SVG: {text}",
        svg.display()
    );
}

#[test]
fn mermaid_no_project_discovered() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.generator.mermaid]\nsrc_dirs = [\".\"]\n",
    )
    .unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("0 products"),
        "Should discover 0 products: {}",
        stdout
    );
}
