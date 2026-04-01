use std::fs;
use std::path::Path;
use tempfile::TempDir;
use crate::common::{run_rsconstruct_with_env, tool_available};

#[test]
fn pandoc_valid_file() {
    if !tool_available("pandoc") {
        eprintln!("pandoc not found, skipping test");
        return;
    }

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    // Use HTML format to avoid requiring LaTeX for PDF
    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.pandoc]\nformats = [\"html\"]\nscan_dir = \"\"\n",
    )
    .unwrap();

    fs::write(
        project_path.join("doc.md"),
        "# Hello World\n\nThis is a test document.\n",
    )
    .unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build", "-v"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "Build should succeed with valid markdown for pandoc: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Processing:"),
        "Should process pandoc: {}",
        stdout
    );
}

#[test]
fn pandoc_incremental_skip() {
    if !tool_available("pandoc") {
        eprintln!("pandoc not found, skipping test");
        return;
    }

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.pandoc]\nformats = [\"html\"]\nscan_dir = \"\"\n",
    )
    .unwrap();

    fs::write(
        project_path.join("doc.md"),
        "# Hello World\n\nThis is a test.\n",
    )
    .unwrap();

    // First build
    let output1 = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output1.status.success());

    // Second build should skip
    let output2 = run_rsconstruct_with_env(project_path, &["build", "--verbose"], &[("NO_COLOR", "1")]);
    assert!(output2.status.success());
    let stdout2 = String::from_utf8_lossy(&output2.stdout);
    assert!(
        stdout2.contains("[pandoc] Skipping (unchanged):"),
        "Second build should skip: {}",
        stdout2
    );
}

#[test]
fn pandoc_no_project_discovered() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.pandoc]\n",
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

/// Build the same markdown file twice with pandoc and verify the PDF output is binary identical.
/// This tests that our pandoc invocation is deterministic (no embedded timestamps, random IDs, etc.).
#[test]
fn pandoc_pdf_deterministic() {
    if !tool_available("pandoc") || !tool_available("pdflatex") {
        eprintln!("pandoc or pdflatex not found, skipping test");
        return;
    }

    let md_content = "---\ntitle: Determinism Test\n---\n# Hello World\n\nThis is a test.\n\n## Outline\n* Chapter one\n* Chapter two\n";
    let config = "[processor.pandoc]\nformats = [\"pdf\"]\nscan_dir = \"\"\n";

    // Build 1
    let dir1 = TempDir::new().expect("Failed to create temp dir");
    fs::write(dir1.path().join("rsconstruct.toml"), config).unwrap();
    fs::write(dir1.path().join("doc.md"), md_content).unwrap();
    let out1 = run_rsconstruct_with_env(dir1.path(), &["build"], &[("NO_COLOR", "1")]);
    assert!(out1.status.success(), "Build 1 failed: {}", String::from_utf8_lossy(&out1.stderr));

    // Build 2
    let dir2 = TempDir::new().expect("Failed to create temp dir");
    fs::write(dir2.path().join("rsconstruct.toml"), config).unwrap();
    fs::write(dir2.path().join("doc.md"), md_content).unwrap();
    let out2 = run_rsconstruct_with_env(dir2.path(), &["build"], &[("NO_COLOR", "1")]);
    assert!(out2.status.success(), "Build 2 failed: {}", String::from_utf8_lossy(&out2.stderr));

    // Compare PDFs
    let pdf1 = fs::read(dir1.path().join("out/pandoc/doc.pdf")).expect("PDF 1 not found");
    let pdf2 = fs::read(dir2.path().join("out/pandoc/doc.pdf")).expect("PDF 2 not found");
    assert_eq!(pdf1, pdf2, "PDFs should be binary identical but differ ({} vs {} bytes)", pdf1.len(), pdf2.len());
}
