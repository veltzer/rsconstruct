use std::fs;
use tempfile::TempDir;

use crate::common::{require_tool, run_rsconstruct_with_env};

// The tool named here is probed by running it, and a jar does not run by
// itself: java is the probe, and a missing vnu.jar fails the build below.
test_checker!(vnu, tool: "java", processor: "processor.checker.vnu",
    files: [("test.html", "<!DOCTYPE html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><title>Test</title></head><body><main><p>Hello</p></main></body></html>\n")]);

/// The reason the processor exists: a `<main>` inside a `<main>` breaks
/// the HTML standard's content model, which tidy does not check.
#[test]
fn vnu_rejects_a_nested_main() {
    require_tool("java");

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();
    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.checker.vnu]\nsrc_dirs = [\".\"]\n",
    )
    .unwrap();
    fs::write(
        project_path.join("nested.html"),
        "<!DOCTYPE html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><title>Test</title></head><body><main><main><p>Hello</p></main></main></body></html>\n",
    )
    .unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(
        !output.status.success(),
        "a nested <main> must fail the build"
    );
    let report = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(report.contains("main"), "{report}");
}
