use crate::common::run_rsconstruct_with_env;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

const CONFIG: &str = "[processor.checker.iactionlint]\nsrc_dirs = [\".github/workflows\"]\n";

/// A project with one workflow file under `.github/workflows`.
fn setup_project_with(config: &str, workflow: &str) -> TempDir {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    fs::write(temp_dir.path().join("rsconstruct.toml"), config)
        .expect("Failed to write rsconstruct.toml");
    fs::create_dir_all(temp_dir.path().join(".github/workflows")).unwrap();
    fs::write(temp_dir.path().join(".github/workflows/ci.yml"), workflow)
        .expect("Failed to write workflow");
    temp_dir
}

fn setup_project(workflow: &str) -> TempDir {
    setup_project_with(CONFIG, workflow)
}

fn build(project_path: &Path) -> std::process::Output {
    run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")])
}

fn combined_output(output: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn assert_passes(output: &std::process::Output, what: &str) {
    assert!(
        output.status.success(),
        "{what} must pass: {}",
        combined_output(output)
    );
}

fn assert_fails_with(output: &std::process::Output, what: &str, needle: &str) {
    assert!(!output.status.success(), "{what} must fail");
    let text = combined_output(output);
    assert!(
        text.contains(needle),
        "{what}: {needle:?} not reported: {text}"
    );
}

const VALID: &str = "name: CI\non:\n  push:\n    branches: [master]\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: actions/checkout@v4\n      - run: echo ${{ github.sha }}\n";

#[test]
fn iactionlint_valid_workflow() {
    let temp_dir = setup_project(VALID);
    assert_passes(&build(temp_dir.path()), "a valid workflow");
}

#[test]
fn iactionlint_reports_in_actionlint_format() {
    let temp_dir = setup_project(
        "on: push\njobs:\n  test:\n    runs-on: ubuntu-lates\n    steps:\n      - run: echo ${{ github.foo.bar }}\n",
    );
    let output = build(temp_dir.path());
    assert_fails_with(
        &output,
        "an unknown runner label",
        ".github/workflows/ci.yml:4:14: label \"ubuntu-lates\" is unknown.",
    );
    assert!(
        combined_output(&output).contains(
            ".github/workflows/ci.yml:6:23: property \"foo\" is not defined in object type"
        ),
        "the expression error must be reported too: {}",
        combined_output(&output)
    );
    assert!(combined_output(&output).contains("[runner-label]"));
    assert!(combined_output(&output).contains("[expression]"));
}

#[test]
fn iactionlint_syntax_errors() {
    let temp_dir = setup_project("on: push\njobs:\n  test:\n    steps:\n      - run: echo\n");
    assert_fails_with(
        &build(temp_dir.path()),
        "a job without runs-on",
        ".github/workflows/ci.yml:3:3: \"runs-on\" section is missing in job \"test\" [syntax-check]",
    );
}

/// `.github/actionlint.yaml` is read the way actionlint reads it.
#[test]
fn iactionlint_honours_the_project_config() {
    let workflow =
        "on: push\njobs:\n  test:\n    runs-on: my-runner\n    steps:\n      - run: echo\n";
    let temp_dir = setup_project(workflow);
    assert_fails_with(
        &build(temp_dir.path()),
        "a self-hosted label without config",
        "label \"my-runner\" is unknown",
    );

    let temp_dir = setup_project(workflow);
    fs::write(
        temp_dir.path().join(".github/actionlint.yaml"),
        "self-hosted-runner:\n  labels: [my-runner]\n",
    )
    .unwrap();
    assert_passes(
        &build(temp_dir.path()),
        "a self-hosted label listed in the config",
    );
}

#[test]
fn iactionlint_config_file_field() {
    let temp_dir = setup_project_with(
        "[processor.checker.iactionlint]\nsrc_dirs = [\".github/workflows\"]\nconfig_file = \"lint/actionlint.yaml\"\n",
        "on: push\njobs:\n  test:\n    runs-on: my-runner\n    steps:\n      - run: echo\n",
    );
    fs::create_dir_all(temp_dir.path().join("lint")).unwrap();
    fs::write(
        temp_dir.path().join("lint/actionlint.yaml"),
        "self-hosted-runner:\n  labels: [my-runner]\n",
    )
    .unwrap();
    assert_passes(&build(temp_dir.path()), "a config named by config_file");
}

/// Popular actions' inputs are known without any network access.
#[test]
fn iactionlint_checks_popular_action_inputs() {
    let temp_dir = setup_project(
        "on: push\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: actions/checkout@v4\n        with:\n          fetch-deph: 0\n",
    );
    assert_fails_with(
        &build(temp_dir.path()),
        "a misspelled input",
        "input \"fetch-deph\" is not defined in action \"actions/checkout@v4\"",
    );
}

/// Local actions are read from the project root.
#[test]
fn iactionlint_checks_local_actions() {
    let temp_dir = setup_project(
        "on: push\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: ./.github/actions/hello\n        with:\n          name: x\n",
    );
    fs::create_dir_all(temp_dir.path().join(".github/actions/hello")).unwrap();
    fs::write(
        temp_dir.path().join(".github/actions/hello/action.yml"),
        "name: Hello\ndescription: Says hello\ninputs:\n  who:\n    required: true\nruns:\n  using: composite\n  steps:\n    - run: echo hi\n      shell: bash\n",
    )
    .unwrap();
    let output = build(temp_dir.path());
    assert_fails_with(
        &output,
        "an undefined input of a local action",
        "input \"name\" is not defined in action \"Hello\" defined at \"./.github/actions/hello\". available inputs are \"who\" [action]",
    );
    assert!(
        combined_output(&output)
            .contains("missing input \"who\" which is required by action \"Hello\""),
        "{}",
        combined_output(&output)
    );
}
