use crate::common::{require_tool, run_rsconstruct_with_env};
use std::fs;
use tempfile::TempDir;

fn project(tex: &str) -> TempDir {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    fs::write(
        temp_dir.path().join("rsconstruct.toml"),
        "[processor.generator.tectonic]\nsrc_dirs = [\".\"]\n",
    )
    .unwrap();
    fs::write(temp_dir.path().join("doc.tex"), tex).unwrap();
    temp_dir
}

const DOC: &str = "\\documentclass{article}\n\\begin{document}\nHello World\n\\end{document}\n";

/// A document is typeset to `<output_dir>/doc.pdf`, nothing else is left
/// beside it, and the same source gives the same bytes on a rebuild.
#[test]
fn tectonic_valid_file_is_reproducible() {
    require_tool("tectonic");

    let temp_dir = project(DOC);
    let project_path = temp_dir.path();
    let pdf = project_path.join("out/processor.generator.tectonic/doc.pdf");

    let build = || {
        let output = run_rsconstruct_with_env(project_path, &["build", "-v"], &[("NO_COLOR", "1")]);
        assert!(
            output.status.success(),
            "Build should succeed: stdout={}, stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        fs::read(&pdf).expect("doc.pdf is written")
    };
    let first = build();
    assert!(first.starts_with(b"%PDF"));
    let left: Vec<_> = fs::read_dir(pdf.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(left, vec![std::ffi::OsString::from("doc.pdf")]);

    let cleaned = run_rsconstruct_with_env(project_path, &["cache", "clear"], &[("NO_COLOR", "1")]);
    assert!(cleaned.status.success());
    fs::remove_file(&pdf).unwrap();
    assert_eq!(build(), first, "a rebuild must give the same PDF");
}

#[test]
fn tectonic_invalid_file_fails() {
    require_tool("tectonic");

    let temp_dir =
        project("\\documentclass{article}\n\\begin{document}\n\\nosuchmacro\n\\end{document}\n");
    let output = run_rsconstruct_with_env(temp_dir.path(), &["build"], &[("NO_COLOR", "1")]);
    assert!(!output.status.success(), "an undefined macro must fail");
    assert!(
        !temp_dir
            .path()
            .join("out/processor.generator.tectonic/doc.pdf")
            .exists()
    );
}

#[test]
fn tectonic_no_project_discovered() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();
    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.generator.tectonic]\nsrc_dirs = [\".\"]\n",
    )
    .unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("0 products"), "{stdout}");
}
