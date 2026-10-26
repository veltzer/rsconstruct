use crate::common::{require_tool, run_rsconstruct_with_env};
use std::fs;
use tempfile::TempDir;

#[test]
fn pdfunite_no_project_discovered() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.generator.pdfunite]\nsrc_dirs = [\".\"]\n",
    )
    .unwrap();

    // No source directory — should succeed with nothing to build
    let output = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("0 products"),
        "Should discover 0 products: {}",
        stdout
    );
}

#[test]
fn pdfunite_discovers_courses() {
    require_tool("pdfunite");

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.generator.pdfunite]\nsrc_dirs = [\".\"]\n",
    )
    .unwrap();

    // Create the source directory structure that pdfunite scans
    fs::create_dir_all(project_path.join("marp/courses/intro")).unwrap();
    fs::write(
        project_path.join("marp/courses/intro/lesson1.md"),
        "# Lesson 1\n",
    )
    .unwrap();
    fs::write(
        project_path.join("marp/courses/intro/lesson2.md"),
        "# Lesson 2\n",
    )
    .unwrap();

    // Dry run: discovery alone, before any PDF exists.
    let output =
        run_rsconstruct_with_env(project_path, &["build", "--dry-run"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "Dry run should succeed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    // The per-lesson PDFs the upstream marp processor would have written
    // (marp is not configured here, so the test supplies them).
    let marp_out = project_path.join("out/processor.generator.marp/courses/intro");
    fs::create_dir_all(&marp_out).unwrap();
    fs::write(marp_out.join("lesson1.pdf"), minimal_pdf()).unwrap();
    fs::write(marp_out.join("lesson2.pdf"), minimal_pdf()).unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "Build should merge the course PDFs: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let merged = project_path.join("out/processor.generator.pdfunite/intro.pdf");
    let bytes =
        fs::read(&merged).unwrap_or_else(|e| panic!("{} not produced: {e}", merged.display()));
    assert!(
        bytes.starts_with(b"%PDF-"),
        "{} is not a PDF",
        merged.display()
    );
    let doc = lopdf::Document::load_mem(&bytes)
        .unwrap_or_else(|e| panic!("{} does not parse: {e}", merged.display()));
    assert_eq!(
        doc.get_pages().len(),
        2,
        "the merged PDF should hold one page per lesson"
    );
}

/// A minimal valid one-page PDF, with a correct cross-reference table.
fn minimal_pdf() -> Vec<u8> {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>",
    ];
    let mut out = String::from("%PDF-1.4\n");
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.push_str(&format!("{} 0 obj\n{body}\nendobj\n", i + 1));
    }
    let xref = out.len();
    out.push_str(&format!(
        "xref\n0 {}\n0000000000 65535 f \n",
        objects.len() + 1
    ));
    for offset in offsets {
        out.push_str(&format!("{offset:010} 00000 n \n"));
    }
    out.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        objects.len() + 1
    ));
    out.into_bytes()
}
