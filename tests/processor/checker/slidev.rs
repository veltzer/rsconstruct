use crate::common::{require_tool, run_rsconstruct_with_env};
use std::fs;
use tempfile::TempDir;

test_checker!(slidev, tool: "slidev", processor: "processor.checker.slidev", no_project);

/// The real `slidev build` on a two-slide deck. `theme: none` keeps the deck
/// independent of which theme packages happen to be installed next to the
/// CLI; the default theme is a separate npm package.
#[test]
fn slidev_builds_a_real_deck() {
    require_tool("slidev");
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.checker.slidev]\nsrc_dirs = [\".\"]\n",
    )
    .unwrap();
    fs::write(
        project_path.join("slides.md"),
        "---\ntheme: none\n---\n\n# Hello\n\n---\n\n# Second\n",
    )
    .unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build", "-v"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "slidev build should succeed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        project_path.join("dist/index.html").is_file(),
        "slidev build should have written dist/index.html"
    );

    let output = run_rsconstruct_with_env(project_path, &["build", "-v"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("[processor.checker.slidev] Skipping (unchanged):"),
        "second build should skip: {stdout}"
    );
}
