use crate::common::{require_tool, run_rsconstruct_with_env};
use std::fs;
use tempfile::TempDir;

test_checker!(selene, tool: "selene", processor: "processor.checker.selene",
    files: [("test.lua", "local x = 1\nprint(x)\n")]);

/// selene exits non-zero on warnings, not only on errors, so a file whose
/// only finding is a warning (an unused local) fails its product.
#[test]
fn selene_warning_fails() {
    require_tool("selene");

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.checker.selene]\nsrc_dirs = [\".\"]\n",
    )
    .unwrap();

    fs::write(project_path.join("test.lua"), "local x = 1\n").unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "Build should fail on an unused local: stdout={stdout}, stderr={stderr}"
    );
    assert!(
        format!("{stdout}{stderr}").contains("unused_variable"),
        "Failure should name selene's lint: stdout={stdout}, stderr={stderr}"
    );
}
