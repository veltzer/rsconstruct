use std::fs;
use tempfile::TempDir;
use crate::common::{run_rsconstruct_with_env, tool_available};

#[test]
fn checkstyle_valid_java() {
    if !tool_available("checkstyle") {
        eprintln!("checkstyle not found, skipping test");
        return;
    }

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor]\nenabled = [\"checkstyle\"]\n\n[processor.checkstyle]\nargs = [\"-c\", \"/google_checks.xml\"]\n",
    )
    .unwrap();

    fs::write(
        project_path.join("Test.java"),
        "public class Test {\n    public static void main(String[] args) {\n    }\n}\n",
    )
    .unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build", "-v"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "Build should succeed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn checkstyle_incremental_skip() {
    if !tool_available("checkstyle") {
        eprintln!("checkstyle not found, skipping test");
        return;
    }

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor]\nenabled = [\"checkstyle\"]\n\n[processor.checkstyle]\nargs = [\"-c\", \"/google_checks.xml\"]\n",
    )
    .unwrap();

    fs::write(
        project_path.join("Test.java"),
        "public class Test {\n    public static void main(String[] args) {\n    }\n}\n",
    )
    .unwrap();

    let output1 = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output1.status.success());

    let output2 = run_rsconstruct_with_env(project_path, &["build", "--verbose"], &[("NO_COLOR", "1")]);
    assert!(output2.status.success());
    let stdout2 = String::from_utf8_lossy(&output2.stdout);
    assert!(
        stdout2.contains("[checkstyle] Skipping (unchanged):"),
        "Second build should skip: {}",
        stdout2
    );
}
