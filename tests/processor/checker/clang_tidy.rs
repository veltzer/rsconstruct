use crate::common::{require_tool, run_rsconstruct_with_env};
use std::fs;
use tempfile::TempDir;

/// clang-tidy finds `.clang-tidy` in the source file's parent directories,
/// so the project root's copy is an input when it exists. Not
/// `compile_commands.json`: the processor always passes `--`, which makes
/// clang-tidy use that fixed compilation database instead.
#[test]
fn clang_tidy_default_dep_auto_tracks_clang_tidy_file() {
    let defconfig = crate::common::processor_defconfig("processor.checker.clang_tidy");
    assert_eq!(defconfig["dep_auto"], serde_json::json!([".clang-tidy"]));
}

#[test]
fn clang_tidy_valid_c_file() {
    require_tool("clang-tidy");

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::create_dir_all(project_path.join("src")).unwrap();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.checker.clang_tidy]\nsrc_dirs = [\"src\"]\n",
    )
    .unwrap();

    fs::write(
        project_path.join("src/main.c"),
        "#include <stdio.h>\nint main(void) {\n    printf(\"hello\\n\");\n    return 0;\n}\n",
    )
    .unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build", "-v"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "Build should succeed with valid C file: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Processing:") || stdout.contains("Processing batch:"),
        "Should process clang_tidy: {}",
        stdout
    );
}

#[test]
fn clang_tidy_incremental_skip() {
    require_tool("clang-tidy");

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::create_dir_all(project_path.join("src")).unwrap();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.checker.clang_tidy]\nsrc_dirs = [\"src\"]\n",
    )
    .unwrap();

    fs::write(
        project_path.join("src/main.c"),
        "#include <stdio.h>\nint main(void) {\n    printf(\"hello\\n\");\n    return 0;\n}\n",
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
        stdout2.contains("[processor.checker.clang_tidy] Skipping (unchanged):"),
        "Second build should skip: {}",
        stdout2
    );
}
