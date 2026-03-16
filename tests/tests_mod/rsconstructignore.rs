use std::fs;
use tempfile::TempDir;
use crate::common::{setup_test_project, setup_cc_project, run_rsconstruct_with_env};

#[test]
fn rsconstructignore_excludes_sleep_files() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create sleep directory with two files
    fs::create_dir_all(project_path.join("sleep")).unwrap();
    fs::write(project_path.join("sleep/included.sleep"), "0.01").unwrap();
    fs::write(project_path.join("sleep/excluded.sleep"), "0.01").unwrap();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor]\nenabled = [\"sleep\"]\n"
    ).unwrap();

    // Create .rsconstructignore that excludes one file
    fs::write(
        project_path.join(".rsconstructignore"),
        "sleep/excluded.sleep\n"
    ).unwrap();

    // Build
    let output = run_rsconstruct_with_env(project_path, &["build", "-v"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(), "rsconstruct build failed: {}", String::from_utf8_lossy(&output.stderr));

    // Verify via output - only included file should be processed
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("included.sleep"), "Included sleep file should be processed");
    assert!(!stdout.contains("excluded.sleep"), "Excluded sleep file should not be processed");
}

#[test]
fn rsconstructignore_glob_pattern() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create sleep directory with subdirectory
    fs::create_dir_all(project_path.join("sleep/subdir")).unwrap();
    fs::write(project_path.join("sleep/keep.sleep"), "0.01").unwrap();
    fs::write(project_path.join("sleep/subdir/skip1.sleep"), "0.01").unwrap();
    fs::write(project_path.join("sleep/subdir/skip2.sleep"), "0.01").unwrap();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor]\nenabled = [\"sleep\"]\n"
    ).unwrap();

    // Use a glob pattern to exclude the entire subdirectory
    fs::write(
        project_path.join(".rsconstructignore"),
        "# Exclude all files in subdir\nsleep/subdir/**\n"
    ).unwrap();

    // Build
    let output = run_rsconstruct_with_env(project_path, &["build", "-v"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(), "rsconstruct build failed: {}", String::from_utf8_lossy(&output.stderr));

    // Verify via output - keep.sleep should be processed, subdir files should not
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("keep.sleep"), "keep.sleep should be processed");
    assert!(!stdout.contains("skip1.sleep"), "subdir/skip1.sleep should be excluded");
    assert!(!stdout.contains("skip2.sleep"), "subdir/skip2.sleep should be excluded");
}

#[test]
fn rsconstructignore_no_file() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create sleep directory — no .rsconstructignore
    fs::create_dir_all(project_path.join("sleep")).unwrap();
    fs::write(project_path.join("sleep/normal.sleep"), "0.01").unwrap();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor]\nenabled = [\"sleep\"]\n"
    ).unwrap();

    // Build should work fine without .rsconstructignore
    let output = run_rsconstruct_with_env(project_path, &["build", "-v"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(), "rsconstruct build failed without .rsconstructignore: {}",
        String::from_utf8_lossy(&output.stderr));

    // Verify via output that the file was processed
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("normal.sleep"),
        "Sleep file should be processed when no .rsconstructignore exists: {}", stdout);
}

#[test]
fn rsconstructignore_comments_and_blank_lines() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    fs::create_dir_all(project_path.join("sleep")).unwrap();
    fs::write(project_path.join("sleep/a.sleep"), "0.01").unwrap();
    fs::write(project_path.join("sleep/b.sleep"), "0.01").unwrap();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor]\nenabled = [\"sleep\"]\n"
    ).unwrap();

    // .rsconstructignore with comments, blank lines, and one real pattern
    fs::write(
        project_path.join(".rsconstructignore"),
        "# This is a comment\n\n   \n# Another comment\nsleep/b.sleep\n\n"
    ).unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build", "-v"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());

    // Verify via output
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("a.sleep"), "a.sleep should be processed");
    assert!(!stdout.contains("b.sleep"), "b.sleep should be ignored");
}

#[test]
fn rsconstructignore_cc_processor() {
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
        project_path.join(".rsconstructignore"),
        "src/excluded/**\n"
    ).unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(),
        "Build failed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr));

    assert!(project_path.join("out/cc_single_file/src/included.elf").exists(),
        "included.c should be compiled");
    assert!(!project_path.join("out/cc_single_file/src/excluded/skip.elf").exists(),
        "excluded/skip.c should not be compiled");
}

#[test]
fn rsconstructignore_leading_slash() {
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
        project_path.join(".rsconstructignore"),
        "/src/skip_dir/**\n"
    ).unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(),
        "Build failed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr));

    assert!(project_path.join("out/cc_single_file/src/keep.elf").exists(),
        "keep.c should be compiled");
    assert!(!project_path.join("out/cc_single_file/src/skip_dir/skip.elf").exists(),
        "skip_dir/skip.c should be excluded by /src/skip_dir/** pattern");
}

#[test]
fn rsconstructignore_trailing_slash() {
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
        project_path.join(".rsconstructignore"),
        "/src/skipme/\n"
    ).unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(),
        "Build failed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr));

    assert!(project_path.join("out/cc_single_file/src/keep.elf").exists(),
        "keep.c should be compiled");
    assert!(!project_path.join("out/cc_single_file/src/skipme/deep.elf").exists(),
        "skipme/deep.c should be excluded by /src/skipme/ pattern");
}
