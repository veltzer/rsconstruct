use crate::common::{require_tool, run_rsconstruct_with_env};
use std::fs;
use tempfile::TempDir;

fn setup_pytest_project() -> TempDir {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    fs::create_dir_all(temp_dir.path().join("tests")).expect("Failed to create tests dir");
    fs::write(
        temp_dir.path().join("rsconstruct.toml"),
        "[processor.checker.pytest]\nsrc_dirs = [\"tests\"]\n",
    )
    .expect("Failed to write rsconstruct.toml");
    temp_dir
}

#[test]
fn pytest_passing_test() {
    require_tool("pytest");

    let temp_dir = setup_pytest_project();
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("tests/test_example.py"),
        "def test_addition():\n    assert 1 + 1 == 2\n",
    )
    .unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build", "-v"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "Build should succeed with passing test: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Processing:"),
        "Should process pytest: {}",
        stdout
    );
}

#[test]
fn pytest_incremental_skip() {
    require_tool("pytest");

    let temp_dir = setup_pytest_project();
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("tests/test_skip.py"),
        "def test_trivial():\n    assert True\n",
    )
    .unwrap();

    let output1 = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output1.status.success());

    let output2 =
        run_rsconstruct_with_env(project_path, &["build", "--verbose"], &[("NO_COLOR", "1")]);
    assert!(output2.status.success());
    let stdout2 = String::from_utf8_lossy(&output2.stdout);
    assert!(
        stdout2.contains("[processor.checker.pytest] Skipping (unchanged):"),
        "Second build should skip: {}",
        stdout2
    );
}

#[test]
fn pytest_failing_test() {
    require_tool("pytest");

    let temp_dir = setup_pytest_project();
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("tests/test_fail.py"),
        "def test_failure():\n    assert 1 == 2\n",
    )
    .unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(
        !output.status.success(),
        "Build should fail with failing test"
    );
}

/// A `conftest.py` beside the tests is an input of each of them and not a
/// test itself. It used to be scanned as a product (pytest collects nothing
/// from it and exits 5), and a fixture changed in it left the cached test
/// results in place: a test the change broke stayed green.
#[test]
fn pytest_conftest_is_an_input_of_the_tests_below_it() {
    require_tool("pytest");

    let temp_dir = setup_pytest_project();
    let project_path = temp_dir.path();
    let conftest = project_path.join("tests/conftest.py");
    let fixture = |value: u32| {
        format!("import pytest\n\n\n@pytest.fixture\ndef number():\n    return {value}\n")
    };
    fs::write(&conftest, fixture(1)).unwrap();
    fs::write(
        project_path.join("tests/test_number.py"),
        "def test_number(number):\n    assert number == 1\n",
    )
    .unwrap();

    let first = run_rsconstruct_with_env(project_path, &["build", "-v"], &[("NO_COLOR", "1")]);
    let stdout = String::from_utf8_lossy(&first.stdout);
    assert!(
        first.status.success(),
        "{stdout}{}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert!(
        !stdout.contains("conftest.py"),
        "conftest.py must not be run as a test: {stdout}"
    );

    // A value of another length: Python holds a `.pyc` to its source by
    // size and whole-second mtime, so a same-size rewrite within the second
    // ran the stale bytecode and the test passed on the old fixture.
    fs::write(&conftest, fixture(20)).unwrap();
    let second = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(
        !second.status.success(),
        "the fixture change breaks the test, and the build must see it"
    );
}
