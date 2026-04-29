use std::fs;
use tempfile::TempDir;
use crate::common::run_rsconstruct_with_env;

/// Baseline: imports get derived to a sorted requirements.txt.
#[test]
fn requirements_derives_from_imports() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.requirements]\nsrc_dirs = [\".\"]\n",
    )
    .unwrap();

    fs::write(
        project_path.join("app.py"),
        "import requests\nimport flask\n",
    )
    .unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "Build should succeed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let req = fs::read_to_string(project_path.join("requirements.txt")).unwrap();
    assert!(req.contains("flask"), "expected flask in output: {}", req);
    assert!(req.contains("requests"), "expected requests in output: {}", req);
}

/// `extra` adds distributions that no `import` references — the use case
/// is upstream packages that need a runtime dep at import time but fail
/// to declare it in their metadata (e.g. `manim_voiceover` needing
/// `setuptools` for `import pkg_resources`).
#[test]
fn requirements_extra_adds_undeclared_dep() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.requirements]\n\
         src_dirs = [\".\"]\n\
         extra = [\"setuptools\"]\n",
    )
    .unwrap();

    // Note: no `import setuptools` anywhere — that's the whole point.
    fs::write(
        project_path.join("app.py"),
        "import flask\n",
    )
    .unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "Build should succeed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let req = fs::read_to_string(project_path.join("requirements.txt")).unwrap();
    assert!(
        req.contains("setuptools"),
        "extra=[setuptools] should appear in requirements.txt even though no import references it: {}",
        req
    );
    assert!(req.contains("flask"), "import-derived flask should still appear: {}", req);
}

/// `extra` should bypass `exclude` — exclude operates on import names of
/// scanned imports, while extras are distributions explicitly asserted by
/// the user. Listing both is a nonsense config; the user-asserted one wins.
#[test]
fn requirements_extra_bypasses_exclude() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.requirements]\n\
         src_dirs = [\".\"]\n\
         exclude = [\"setuptools\"]\n\
         extra = [\"setuptools\"]\n",
    )
    .unwrap();

    fs::write(project_path.join("app.py"), "import flask\n").unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());

    let req = fs::read_to_string(project_path.join("requirements.txt")).unwrap();
    assert!(
        req.contains("setuptools"),
        "extra should bypass exclude: {}",
        req
    );
}

/// Adding something to `extra` is a config change that should invalidate
/// the cache and trigger a rebuild — `extra` must be in checksum_fields.
#[test]
fn requirements_extra_change_triggers_rebuild() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.requirements]\nsrc_dirs = [\".\"]\n",
    )
    .unwrap();
    fs::write(project_path.join("app.py"), "import flask\n").unwrap();

    let out1 = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(out1.status.success());
    let req1 = fs::read_to_string(project_path.join("requirements.txt")).unwrap();
    assert!(!req1.contains("setuptools"), "round 1 should not have setuptools: {}", req1);

    // Now add extra — the config hash should change, forcing a rebuild.
    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.requirements]\n\
         src_dirs = [\".\"]\n\
         extra = [\"setuptools\"]\n",
    )
    .unwrap();

    let out2 = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(out2.status.success());
    let req2 = fs::read_to_string(project_path.join("requirements.txt")).unwrap();
    assert!(
        req2.contains("setuptools"),
        "round 2 should pick up the new extra: {}",
        req2
    );
}
