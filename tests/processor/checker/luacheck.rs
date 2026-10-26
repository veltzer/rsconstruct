use crate::common::{require_tool, run_rsconstruct_with_env};
use std::fs;
use tempfile::TempDir;

/// luacheck's verdict depends on the file's path (`.luacheckrc` applies
/// options per path, and the default std follows the path), so cached
/// results are keyed by name as well as content — as for iluacheck, whose
/// defaults this processor must match.
#[test]
fn luacheck_default_output_depends_on_input_name() {
    let defconfig = crate::common::processor_defconfig("processor.checker.luacheck");
    assert_eq!(defconfig["output_depends_on_input_name"], true);
    assert_eq!(defconfig["dep_auto"], serde_json::json!([".luacheckrc"]));
    let iluacheck = crate::common::processor_defconfig("processor.checker.iluacheck");
    assert_eq!(
        defconfig["output_depends_on_input_name"],
        iluacheck["output_depends_on_input_name"]
    );
    assert_eq!(defconfig["dep_auto"], iluacheck["dep_auto"]);
}

#[test]
fn luacheck_valid_file() {
    require_tool("luacheck");

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.checker.luacheck]\nsrc_dirs = [\".\"]\n",
    )
    .unwrap();

    fs::write(project_path.join("test.lua"), "local x = 1\nprint(x)\n").unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build", "-v"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "Build should succeed with valid Lua file: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Processing:"),
        "Should process luacheck: {}",
        stdout
    );
}

#[test]
fn luacheck_incremental_skip() {
    require_tool("luacheck");

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.checker.luacheck]\nsrc_dirs = [\".\"]\n",
    )
    .unwrap();

    fs::write(project_path.join("test.lua"), "local x = 1\nprint(x)\n").unwrap();

    // First build
    let output1 = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output1.status.success());

    // Second build should skip
    let output2 =
        run_rsconstruct_with_env(project_path, &["build", "--verbose"], &[("NO_COLOR", "1")]);
    assert!(output2.status.success());
    let stdout2 = String::from_utf8_lossy(&output2.stdout);
    assert!(
        stdout2.contains("[processor.checker.luacheck] Skipping (unchanged):"),
        "Second build should skip: {}",
        stdout2
    );
}

#[test]
fn luacheck_no_project_discovered() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.checker.luacheck]\nsrc_dirs = [\".\"]\n",
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
