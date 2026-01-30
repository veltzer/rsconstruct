mod common;

use std::fs;
use tempfile::TempDir;
use common::{setup_test_project, setup_cc_project, run_rsb_with_env};

#[test]
fn rsbignore_excludes_sleep_files() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create sleep directory with two files
    fs::create_dir_all(project_path.join("sleep")).unwrap();
    fs::write(project_path.join("sleep/included.sleep"), "0.01").unwrap();
    fs::write(project_path.join("sleep/excluded.sleep"), "0.01").unwrap();

    fs::write(
        project_path.join("rsb.toml"),
        "[processor]\nenabled = [\"sleep\"]\n"
    ).unwrap();

    // Create .rsbignore that excludes one file
    fs::write(
        project_path.join(".rsbignore"),
        "sleep/excluded.sleep\n"
    ).unwrap();

    // Build
    let output = run_rsb_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(), "rsb build failed: {}", String::from_utf8_lossy(&output.stderr));

    // The included file should be processed
    assert!(project_path.join("out/sleep/included.done").exists(),
        "Included sleep file should be processed");

    // The excluded file should NOT be processed
    assert!(!project_path.join("out/sleep/excluded.done").exists(),
        "Excluded sleep file should not be processed");
}

#[test]
fn rsbignore_glob_pattern() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create sleep directory with subdirectory
    fs::create_dir_all(project_path.join("sleep/subdir")).unwrap();
    fs::write(project_path.join("sleep/keep.sleep"), "0.01").unwrap();
    fs::write(project_path.join("sleep/subdir/skip1.sleep"), "0.01").unwrap();
    fs::write(project_path.join("sleep/subdir/skip2.sleep"), "0.01").unwrap();

    fs::write(
        project_path.join("rsb.toml"),
        "[processor]\nenabled = [\"sleep\"]\n"
    ).unwrap();

    // Use a glob pattern to exclude the entire subdirectory
    fs::write(
        project_path.join(".rsbignore"),
        "# Exclude all files in subdir\nsleep/subdir/**\n"
    ).unwrap();

    // Build
    let output = run_rsb_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(), "rsb build failed: {}", String::from_utf8_lossy(&output.stderr));

    // keep.sleep should be processed
    assert!(project_path.join("out/sleep/keep.done").exists(),
        "keep.sleep should be processed");

    // subdir files should be excluded
    assert!(!project_path.join("out/sleep/skip1.done").exists(),
        "subdir/skip1.sleep should be excluded");
    assert!(!project_path.join("out/sleep/skip2.done").exists(),
        "subdir/skip2.sleep should be excluded");
}

#[test]
fn rsbignore_no_file() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create sleep directory — no .rsbignore
    fs::create_dir_all(project_path.join("sleep")).unwrap();
    fs::write(project_path.join("sleep/normal.sleep"), "0.01").unwrap();

    fs::write(
        project_path.join("rsb.toml"),
        "[processor]\nenabled = [\"sleep\"]\n"
    ).unwrap();

    // Build should work fine without .rsbignore
    let output = run_rsb_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(), "rsb build failed without .rsbignore: {}",
        String::from_utf8_lossy(&output.stderr));

    assert!(project_path.join("out/sleep/normal.done").exists(),
        "Sleep file should be processed when no .rsbignore exists");
}

#[test]
fn rsbignore_comments_and_blank_lines() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    fs::create_dir_all(project_path.join("sleep")).unwrap();
    fs::write(project_path.join("sleep/a.sleep"), "0.01").unwrap();
    fs::write(project_path.join("sleep/b.sleep"), "0.01").unwrap();

    fs::write(
        project_path.join("rsb.toml"),
        "[processor]\nenabled = [\"sleep\"]\n"
    ).unwrap();

    // .rsbignore with comments, blank lines, and one real pattern
    fs::write(
        project_path.join(".rsbignore"),
        "# This is a comment\n\n   \n# Another comment\nsleep/b.sleep\n\n"
    ).unwrap();

    let output = run_rsb_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());

    assert!(project_path.join("out/sleep/a.done").exists(), "a.sleep should be processed");
    assert!(!project_path.join("out/sleep/b.done").exists(), "b.sleep should be ignored");
}

#[test]
fn rsbignore_cc_processor() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    setup_cc_project(project_path);

    // Create two C files
    fs::write(
        project_path.join("src/included.c"),
        "int main() { return 0; }\n"
    ).unwrap();

    fs::create_dir_all(project_path.join("src/excluded")).unwrap();
    fs::write(
        project_path.join("src/excluded/skip.c"),
        "int main() { return 0; }\n"
    ).unwrap();

    // Exclude the subdirectory
    fs::write(
        project_path.join(".rsbignore"),
        "src/excluded/**\n"
    ).unwrap();

    let output = run_rsb_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(),
        "Build failed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr));

    assert!(project_path.join("out/cc/included.elf").exists(),
        "included.c should be compiled");
    assert!(!project_path.join("out/cc/excluded/skip.elf").exists(),
        "excluded/skip.c should not be compiled");
}

#[test]
fn rsbignore_leading_slash() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    setup_cc_project(project_path);

    fs::write(
        project_path.join("src/keep.c"),
        "int main() { return 0; }\n"
    ).unwrap();

    fs::create_dir_all(project_path.join("src/skip_dir")).unwrap();
    fs::write(
        project_path.join("src/skip_dir/skip.c"),
        "int main() { return 0; }\n"
    ).unwrap();

    // Leading '/' should work like .gitignore (anchored to project root)
    fs::write(
        project_path.join(".rsbignore"),
        "/src/skip_dir/**\n"
    ).unwrap();

    let output = run_rsb_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(),
        "Build failed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr));

    assert!(project_path.join("out/cc/keep.elf").exists(),
        "keep.c should be compiled");
    assert!(!project_path.join("out/cc/skip_dir/skip.elf").exists(),
        "skip_dir/skip.c should be excluded by /src/skip_dir/** pattern");
}

#[test]
fn rsbignore_trailing_slash() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    setup_cc_project(project_path);

    fs::write(
        project_path.join("src/keep.c"),
        "int main() { return 0; }\n"
    ).unwrap();

    fs::create_dir_all(project_path.join("src/skipme")).unwrap();
    fs::write(
        project_path.join("src/skipme/deep.c"),
        "int main() { return 0; }\n"
    ).unwrap();

    // Trailing '/' should exclude the directory and all its contents
    fs::write(
        project_path.join(".rsbignore"),
        "/src/skipme/\n"
    ).unwrap();

    let output = run_rsb_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(),
        "Build failed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr));

    assert!(project_path.join("out/cc/keep.elf").exists(),
        "keep.c should be compiled");
    assert!(!project_path.join("out/cc/skipme/deep.elf").exists(),
        "skipme/deep.c should be excluded by /src/skipme/ pattern");
}
