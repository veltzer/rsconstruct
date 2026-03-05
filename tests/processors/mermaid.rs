use std::fs;
use tempfile::TempDir;
use crate::common::{run_rsb_with_env, tool_available};

#[test]
fn mermaid_discovery() {
    if !tool_available("mmdc") {
        eprintln!("mmdc (mermaid-cli) not found, skipping test");
        return;
    }

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsbuild.toml"),
        "[processor]\nenabled = [\"mermaid\"]\n",
    )
    .unwrap();

    fs::write(
        project_path.join("diagram.mmd"),
        "graph TD\n    A --> B\n    B --> C\n",
    )
    .unwrap();

    // Use dry-run to verify discovery (mmdc needs Chrome/Puppeteer for rendering)
    let output = run_rsb_with_env(project_path, &["build", "--dry-run"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "Dry run should succeed with valid Mermaid file: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("BUILD") || stdout.contains("build"),
        "Should discover mermaid product: {}",
        stdout
    );
}

#[test]
fn mermaid_no_project_discovered() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsbuild.toml"),
        "[processor]\nenabled = [\"mermaid\"]\n",
    )
    .unwrap();

    let output = run_rsb_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("0 products"),
        "Should discover 0 products: {}",
        stdout
    );
}
