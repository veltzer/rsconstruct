use crate::common::run_rsconstruct_with_env;
use std::fs;
use tempfile::TempDir;

const CONFIG: &str = "[processor.checker.suppressions]\nsrc_dirs = [\"src\"]\n";

fn project(files: &[(&str, &str)]) -> TempDir {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    fs::write(temp_dir.path().join("rsconstruct.toml"), CONFIG).unwrap();
    fs::create_dir_all(temp_dir.path().join("src")).unwrap();
    for (name, text) in files {
        fs::write(temp_dir.path().join("src").join(name), text).unwrap();
    }
    temp_dir
}

#[test]
fn suppressions_with_a_rule_and_a_reason_pass() {
    let temp_dir = project(&[
        (
            "a.py",
            "x = f()  # noqa: E731  teaching: a lambda on purpose\n",
        ),
        (
            "b.c",
            "// teaching: the crash is the example\n// cppcheck-suppress nullPointer\n*p=0;\n",
        ),
    ]);
    let output = run_rsconstruct_with_env(temp_dir.path(), &["build"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A suppression with no rule and one with no reason each fail their own
/// file, with the line and what is missing.
#[test]
fn suppressions_without_a_rule_or_a_reason_fail() {
    let temp_dir = project(&[
        ("a.py", "import os\nx = f()  # noqa\n"),
        (
            "b.sh",
            "#!/bin/bash\necho hi\n# shellcheck disable=SC2034\nx=1\n",
        ),
    ]);
    let output = run_rsconstruct_with_env(
        temp_dir.path(),
        &["build", "--keep-going"],
        &[("NO_COLOR", "1")],
    );
    assert!(!output.status.success());
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(all.contains("src/a.py:2: `noqa` names no rule"), "{all}");
    assert!(
        all.contains("src/b.sh:3: `shellcheck disable` gives no reason"),
        "{all}"
    );
}
