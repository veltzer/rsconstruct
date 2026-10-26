use crate::common::{require_tool, run_rsconstruct_with_env};
use std::fs;
use tempfile::TempDir;

#[test]
fn gem_valid_project() {
    require_tool("bundle");

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.mass_generator.gem]\nsrc_dirs = [\".\"]\n",
    )
    .unwrap();

    // Exclude gems from file index
    fs::write(project_path.join(".rsconstructignore"), "gems/\n").unwrap();

    fs::write(
        project_path.join("Gemfile"),
        "source 'https://rubygems.org'\ngem 'rake'\n",
    )
    .unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build", "-v"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "Build should succeed with valid Gemfile: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Processing:"),
        "Should process gem: {}",
        stdout
    );
}

#[test]
fn gem_incremental_skip() {
    require_tool("bundle");

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.mass_generator.gem]\nsrc_dirs = [\".\"]\n",
    )
    .unwrap();

    // Ignore the installed gems and the Gemfile.lock the first install
    // writes: a lockfile that appears is a new input, which would (rightly)
    // re-run the install once; this test is about an unchanged project.
    fs::write(
        project_path.join(".rsconstructignore"),
        "gems/\nGemfile.lock\n",
    )
    .unwrap();

    fs::write(
        project_path.join("Gemfile"),
        "source 'https://rubygems.org'\ngem 'rake'\n",
    )
    .unwrap();

    // First build
    let output1 = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output1.status.success());

    // Second build should skip
    let output2 =
        run_rsconstruct_with_env(project_path, &["build", "--verbose"], &[("NO_COLOR", "1")]);
    assert!(output2.status.success());
    let stdout2 = String::from_utf8_lossy(&output2.stdout);
    assert!(
        stdout2.contains("[processor.mass_generator.gem] Skipping (unchanged):"),
        "Second build should skip: {}",
        stdout2
    );
}

#[test]
fn gem_no_project_discovered() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.mass_generator.gem]\nsrc_dirs = [\".\"]\n",
    )
    .unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("0 products"),
        "Should discover 0 products: {}",
        stdout
    );
}

/// `Gemfile.lock` is an input of the install stamp: the lockfile the first
/// `bundle install` writes is a new input, so the next build re-runs the
/// install once, after which an unchanged project skips.
#[test]
fn gem_lockfile_is_an_input_of_the_install() {
    require_tool("bundle");

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.mass_generator.gem]\nsrc_dirs = [\".\"]\n",
    )
    .unwrap();
    fs::write(project_path.join(".rsconstructignore"), "gems/\n").unwrap();
    fs::write(
        project_path.join("Gemfile"),
        "source 'https://rubygems.org'\ngem 'rake'\n",
    )
    .unwrap();

    let first = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(
        first.status.success(),
        "first build failed: {}\n{}",
        String::from_utf8_lossy(&first.stdout),
        String::from_utf8_lossy(&first.stderr)
    );
    assert!(
        project_path.join("Gemfile.lock").is_file(),
        "bundle install writes the lockfile"
    );

    let second = run_rsconstruct_with_env(project_path, &["build", "-v"], &[("NO_COLOR", "1")]);
    assert!(second.status.success());
    let stdout = String::from_utf8_lossy(&second.stdout);
    assert!(
        stdout.contains("Processing:"),
        "the new Gemfile.lock is a new input, so the install runs again: {stdout}"
    );

    let third = run_rsconstruct_with_env(project_path, &["build", "-v"], &[("NO_COLOR", "1")]);
    assert!(third.status.success());
    let stdout = String::from_utf8_lossy(&third.stdout);
    assert!(
        stdout.contains("[processor.mass_generator.gem] Skipping (unchanged):"),
        "with the lockfile settled, nothing changes: {stdout}"
    );
}
