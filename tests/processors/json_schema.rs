use std::fs;
use tempfile::TempDir;
use crate::common::run_rsconstruct_with_env;

#[test]
fn json_schema_valid() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.json_schema]\n",
    )
    .unwrap();

    fs::write(
        project_path.join("schema.json"),
        r#"{
  "type": "object",
  "properties": {
    "name": { "type": "string" },
    "age": { "type": "integer" }
  },
  "propertyOrdering": ["name", "age"]
}"#,
    )
    .unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build", "-v"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "Build should succeed with valid schema: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Processing:"),
        "Should process json_schema: {}",
        stdout
    );
}

#[test]
fn json_schema_mismatch() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.json_schema]\n",
    )
    .unwrap();

    fs::write(
        project_path.join("bad.json"),
        r#"{
  "type": "object",
  "properties": {
    "name": { "type": "string" },
    "age": { "type": "integer" }
  },
  "propertyOrdering": ["name"]
}"#,
    )
    .unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build", "-v"], &[("NO_COLOR", "1")]);
    assert!(
        !output.status.success(),
        "Build should fail with mismatched propertyOrdering"
    );
}

#[test]
fn json_schema_incremental_skip() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.json_schema]\n",
    )
    .unwrap();

    fs::write(
        project_path.join("schema.json"),
        r#"{
  "type": "object",
  "properties": {
    "name": { "type": "string" }
  },
  "propertyOrdering": ["name"]
}"#,
    )
    .unwrap();

    // First build
    let output1 = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output1.status.success());

    // Second build should skip
    let output2 = run_rsconstruct_with_env(project_path, &["build", "--verbose"], &[("NO_COLOR", "1")]);
    assert!(output2.status.success());
    let stdout2 = String::from_utf8_lossy(&output2.stdout);
    assert!(
        stdout2.contains("[json_schema] Skipping (unchanged):"),
        "Second build should skip: {}",
        stdout2
    );
}

#[test]
fn json_schema_nested_valid() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.json_schema]\n",
    )
    .unwrap();

    fs::write(
        project_path.join("nested.json"),
        r#"{
  "type": "object",
  "properties": {
    "name": { "type": "string" },
    "address": {
      "type": "object",
      "properties": {
        "street": { "type": "string" },
        "city": { "type": "string" }
      },
      "propertyOrdering": ["street", "city"]
    }
  },
  "propertyOrdering": ["name", "address"]
}"#,
    )
    .unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build", "-v"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "Build should succeed with valid nested schema: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn json_schema_nested_mismatch() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.json_schema]\n",
    )
    .unwrap();

    // Top-level is correct, but nested object has mismatched propertyOrdering
    fs::write(
        project_path.join("nested_bad.json"),
        r#"{
  "type": "object",
  "properties": {
    "name": { "type": "string" },
    "address": {
      "type": "object",
      "properties": {
        "street": { "type": "string" },
        "city": { "type": "string" },
        "zip": { "type": "string" }
      },
      "propertyOrdering": ["street", "city"]
    }
  },
  "propertyOrdering": ["name", "address"]
}"#,
    )
    .unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build", "-v"], &[("NO_COLOR", "1")]);
    assert!(
        !output.status.success(),
        "Build should fail when nested object has mismatched propertyOrdering"
    );
}

#[test]
fn json_schema_no_property_ordering_passes() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.json_schema]\n",
    )
    .unwrap();

    fs::write(
        project_path.join("plain.json"),
        r#"{"name": "test", "values": [1, 2, 3], "nested": {"a": true}}"#,
    )
    .unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build", "-v"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "Build should pass for JSON without propertyOrdering: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
