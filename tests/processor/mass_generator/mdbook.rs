use crate::common::{require_tool, run_rsconstruct_with_env};
use std::fs;
use tempfile::TempDir;

#[test]
fn mdbook_valid_project() {
    require_tool("mdbook");

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.mass_generator.mdbook]\nsrc_dirs = [\".\"]\n",
    )
    .unwrap();

    // Create a minimal mdbook project in a subdirectory
    fs::create_dir_all(project_path.join("docs/src")).unwrap();

    fs::write(
        project_path.join("docs/book.toml"),
        "[book]\ntitle = \"Test\"\n",
    )
    .unwrap();

    fs::write(
        project_path.join("docs/src/SUMMARY.md"),
        "# Summary\n\n- [Chapter 1](./chapter1.md)\n",
    )
    .unwrap();

    fs::write(
        project_path.join("docs/src/chapter1.md"),
        "# Chapter 1\n\nHello world.\n",
    )
    .unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build", "-v"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "Build should succeed with valid mdbook project: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Processing:"),
        "Should process mdbook: {}",
        stdout
    );
}

#[test]
fn mdbook_incremental_skip() {
    require_tool("mdbook");

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.mass_generator.mdbook]\nsrc_dirs = [\".\"]\n",
    )
    .unwrap();

    fs::create_dir_all(project_path.join("docs/src")).unwrap();

    fs::write(
        project_path.join("docs/book.toml"),
        "[book]\ntitle = \"Test\"\n",
    )
    .unwrap();

    fs::write(
        project_path.join("docs/src/SUMMARY.md"),
        "# Summary\n\n- [Chapter 1](./chapter1.md)\n",
    )
    .unwrap();

    fs::write(
        project_path.join("docs/src/chapter1.md"),
        "# Chapter 1\n\nHello world.\n",
    )
    .unwrap();

    // First build
    let output1 = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output1.status.success());

    // Second build should skip
    let output2 =
        run_rsconstruct_with_env(project_path, &["build", "--verbose"], &[("NO_COLOR", "1")]);
    assert!(output2.status.success());
    let stdout2 = String::from_utf8_lossy(&output2.stdout);
    assert!(
        stdout2.contains("[processor.mass_generator.mdbook] Skipping (unchanged):"),
        "Second build should skip: {}",
        stdout2
    );
}

#[test]
fn mdbook_no_project_discovered() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.mass_generator.mdbook]\nsrc_dirs = [\".\"]\n",
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

/// Write a minimal mdbook project whose stanza declares
/// `required_tools = [tool]`.
fn mdbook_project_requiring(tool: &str) -> TempDir {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();
    fs::write(
        project_path.join("rsconstruct.toml"),
        format!(
            "[processor.mass_generator.mdbook]\nsrc_dirs = [\".\"]\nrequired_tools = [\"{tool}\"]\n"
        ),
    )
    .unwrap();
    fs::create_dir_all(project_path.join("docs/src")).unwrap();
    fs::write(
        project_path.join("docs/book.toml"),
        "[book]\ntitle = \"Test\"\n",
    )
    .unwrap();
    fs::write(
        project_path.join("docs/src/SUMMARY.md"),
        "# Summary\n\n- [Chapter 1](./chapter1.md)\n",
    )
    .unwrap();
    fs::write(
        project_path.join("docs/src/chapter1.md"),
        "# Chapter 1\n\nHello world.\n",
    )
    .unwrap();
    temp_dir
}

/// `required_tools` is honoured by the mdbook mass generator: a declared
/// tool that is not on PATH fails the build with the missing-tool error.
#[test]
fn mdbook_required_tools_missing_fails_build() {
    require_tool("mdbook");
    let temp_dir = mdbook_project_requiring("rsconstruct-test-missing-tool");
    let output = run_rsconstruct_with_env(temp_dir.path(), &["build"], &[("NO_COLOR", "1")]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "build must fail: stderr={stderr}");
    assert!(
        stderr.contains("Missing required tools")
            && stderr.contains(
                "rsconstruct-test-missing-tool (needed by: processor.mass_generator.mdbook)"
            ),
        "expected the missing-tool error: stderr={stderr}"
    );
}

/// A declared tool that is on PATH leaves the build unaffected.
#[test]
fn mdbook_required_tools_present_builds() {
    require_tool("mdbook");
    let temp_dir = mdbook_project_requiring("sh");
    let output = run_rsconstruct_with_env(temp_dir.path(), &["build"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "build should succeed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
