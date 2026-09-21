use crate::common::{run_rsconstruct, run_rsconstruct_with_env, setup_test_project};
use std::fs;

#[test]
fn status_command() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create a template
    fs::write(
        project_path.join("tera.templates/status_test.txt.tera"),
        "hello",
    )
    .unwrap();

    // Before building, should be NEW (never built, use -v to see per-file status)
    let status1 = run_rsconstruct_with_env(project_path, &["-v", "status"], &[("NO_COLOR", "1")]);
    assert!(status1.status.success());
    let stdout1 = String::from_utf8_lossy(&status1.stdout);
    assert!(
        stdout1.contains("NEW"),
        "Before build, product should be NEW: {}",
        stdout1
    );

    // Build it
    let build = run_rsconstruct(project_path, &["build"]);
    assert!(build.status.success());

    // After building, should be UP-TO-DATE
    let status2 = run_rsconstruct_with_env(project_path, &["-v", "status"], &[("NO_COLOR", "1")]);
    assert!(status2.status.success());
    let stdout2 = String::from_utf8_lossy(&status2.stdout);
    assert!(
        stdout2.contains("UP-TO-DATE"),
        "After build, product should be UP-TO-DATE: {}",
        stdout2
    );

    // Check summary line
    assert!(
        stdout2.contains("Total"),
        "Status output should contain Total line"
    );
}

#[test]
fn status_empty_project() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // No templates to process — disable all processors
    fs::write(project_path.join("rsconstruct.toml"), "\n").unwrap();

    let output = run_rsconstruct(project_path, &["status"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("No products discovered"),
        "Empty project should show 'No products discovered': {}",
        stdout
    );
}

/// The per-processor status table and its JSON form carry a `rust` flag next
/// to `native`, so the language of the toolchain can be read off `status`.
/// The test project's only processor is tera, which is native and therefore
/// Rust.
#[test]
fn status_reports_rust() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();
    fs::write(
        project_path.join("tera.templates/rust_test.txt.tera"),
        "hello",
    )
    .unwrap();

    let json = run_rsconstruct_with_env(project_path, &["--json", "status"], &[("NO_COLOR", "1")]);
    assert!(
        json.status.success(),
        "status --json failed: {}",
        String::from_utf8_lossy(&json.stderr)
    );
    let stdout = String::from_utf8_lossy(&json.stdout);
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("Expected valid JSON");
    let processors = value["processors"]
        .as_array()
        .expect("status --json should list processors");
    let tera = processors
        .iter()
        .find(|p| p["name"] == "tera")
        .expect("tera processor missing from status --json");
    assert_eq!(tera["native"], true);
    assert_eq!(tera["rust"], true);

    let table = run_rsconstruct_with_env(project_path, &["status"], &[("NO_COLOR", "1")]);
    assert!(table.status.success());
    let table_out = String::from_utf8_lossy(&table.stdout);
    assert!(
        table_out.contains("rust"),
        "status table should have a rust column: {table_out}"
    );
}
