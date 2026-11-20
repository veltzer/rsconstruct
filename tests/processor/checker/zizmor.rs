use crate::common::{require_tool, run_rsconstruct_with_env};
use std::fs;
use tempfile::TempDir;

const CLEAN: &str = "name: CI\non: push\npermissions: {}\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - run: echo hello\n";

test_checker!(zizmor, tool: "zizmor", processor: "processor.checker.zizmor",
    files: [("ci.yml", CLEAN)]);

/// A workflow that interpolates an attacker-controlled value into a shell
/// script is a finding, and a finding fails the product.
#[test]
fn zizmor_template_injection_fails() {
    require_tool("zizmor");

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();
    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.checker.zizmor]\nsrc_dirs = [\".\"]\n",
    )
    .unwrap();
    fs::write(
        project_path.join("ci.yml"),
        "name: CI\non: pull_request_target\npermissions: {}\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - run: echo ${{ github.event.pull_request.title }}\n",
    )
    .unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(!output.status.success(), "an injectable workflow must fail");
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(all.contains("template-injection"), "{all}");
}

/// The default arguments keep the audit offline: a build has no token and,
/// sandboxed, no network.
#[test]
fn zizmor_runs_offline_by_default() {
    let defconfig = crate::common::processor_defconfig("processor.checker.zizmor");
    let args: Vec<&str> = defconfig["args"]
        .as_array()
        .expect("args is a list")
        .iter()
        .map(|v| v.as_str().expect("args entries are strings"))
        .collect();
    assert!(args.contains(&"--offline"), "{args:?}");
}
