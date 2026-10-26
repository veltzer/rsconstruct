use crate::common::{require_tool, run_rsconstruct_with_env};
use std::fs;
use tempfile::TempDir;

#[test]
fn drawio_discovery() {
    require_tool("drawio");

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.generator.drawio]\nsrc_dirs = [\".\"]\n",
    )
    .unwrap();

    // Minimal Draw.io XML
    fs::write(
        project_path.join("diagram.drawio"),
        r#"<mxfile><diagram name="Page-1"><mxGraphModel><root><mxCell id="0"/><mxCell id="1" parent="0"/></root></mxGraphModel></diagram></mxfile>"#,
    )
    .unwrap();

    let output =
        run_rsconstruct_with_env(project_path, &["build", "--dry-run"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "Dry run should succeed with valid drawio file: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("BUILD") || stdout.contains("build"),
        "Should discover drawio product: {}",
        stdout
    );
}

/// A real export of a one-shape diagram to PNG and SVG.
#[test]
fn drawio_builds_png_and_svg() {
    require_tool("drawio");

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.generator.drawio]\nsrc_dirs = [\".\"]\nformats = [\"png\", \"svg\"]\n",
    )
    .unwrap();
    fs::write(
        project_path.join("diagram.drawio"),
        r#"<mxfile><diagram name="Page-1"><mxGraphModel><root><mxCell id="0"/><mxCell id="1" parent="0"/><mxCell id="2" value="Hi" style="rounded=0;" vertex="1" parent="1"><mxGeometry x="10" y="10" width="80" height="40" as="geometry"/></mxCell></root></mxGraphModel></diagram></mxfile>"#,
    )
    .unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "Build should export the drawio file: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let png = project_path.join("out/processor.generator.drawio/diagram.png");
    let bytes = fs::read(&png).unwrap_or_else(|e| panic!("{} not produced: {e}", png.display()));
    assert!(
        bytes.starts_with(b"\x89PNG"),
        "{} is not a PNG",
        png.display()
    );
    let svg = project_path.join("out/processor.generator.drawio/diagram.svg");
    let text =
        fs::read_to_string(&svg).unwrap_or_else(|e| panic!("{} not produced: {e}", svg.display()));
    assert!(text.contains("<svg"), "{} is not an SVG", svg.display());
}

#[test]
fn drawio_no_project_discovered() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.generator.drawio]\nsrc_dirs = [\".\"]\n",
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
