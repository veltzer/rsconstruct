use std::fs;
use crate::common::{setup_test_project, run_rsb, run_rsb_with_env};

#[test]
fn spellcheck_correct_spelling() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create a markdown file with correct spelling
    fs::write(
        project_path.join("README.md"),
        "# Hello World\n\nThis is a simple document with correct spelling.\n"
    ).unwrap();

    fs::write(
        project_path.join("rsb.toml"),
        "[processor]\nenabled = [\"spellcheck\"]\n"
    ).unwrap();

    let output = run_rsb_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(),
        "Build should succeed with correct spelling: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr));

    // Checkers no longer create stub files - success is recorded in cache database
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("[spellcheck] Processing:"),
        "Should process spellcheck: {}", stdout);
}

#[test]
fn spellcheck_misspelled_word() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create a markdown file with a misspelled word
    fs::write(
        project_path.join("README.md"),
        "# Hello World\n\nThis document has a speling error.\n"
    ).unwrap();

    fs::write(
        project_path.join("rsb.toml"),
        "[processor]\nenabled = [\"spellcheck\"]\n"
    ).unwrap();

    let output = run_rsb_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(!output.status.success(),
        "Build should fail with misspelled word");

    let combined = format!("{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr));
    assert!(combined.contains("speling"),
        "Error should mention the misspelled word: {}", combined);
}

#[test]
fn spellcheck_custom_words_file() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create a markdown file with a "misspelled" word that is actually a custom word
    fs::write(
        project_path.join("README.md"),
        "# Hello World\n\nThis uses rsb for building.\n"
    ).unwrap();

    // Add "rsb" to custom words file
    fs::write(
        project_path.join(".spellcheck-words"),
        "# Custom project words\nrsb\n"
    ).unwrap();

    fs::write(
        project_path.join("rsb.toml"),
        "[processor]\nenabled = [\"spellcheck\"]\n\n[processor.spellcheck]\nuse_words_file = true\n"
    ).unwrap();

    let output = run_rsb_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(),
        "Build should succeed with custom words: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr));
}

#[test]
fn spellcheck_incremental_skip() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("README.md"),
        "# Hello World\n\nThis is correct.\n"
    ).unwrap();

    fs::write(
        project_path.join("rsb.toml"),
        "[processor]\nenabled = [\"spellcheck\"]\n"
    ).unwrap();

    // First build
    let output1 = run_rsb_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output1.status.success());
    let stdout1 = String::from_utf8_lossy(&output1.stdout);
    assert!(stdout1.contains("[spellcheck] Processing:"),
        "First build should process: {}", stdout1);

    // Second build should skip
    let output2 = run_rsb_with_env(project_path, &["build", "--verbose"], &[("NO_COLOR", "1")]);
    assert!(output2.status.success());
    let stdout2 = String::from_utf8_lossy(&output2.stdout);
    assert!(stdout2.contains("[spellcheck] Skipping (unchanged):"),
        "Second build should skip: {}", stdout2);
}

#[test]
fn spellcheck_clean() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("README.md"),
        "# Hello World\n\nThis is correct.\n"
    ).unwrap();

    fs::write(
        project_path.join("rsb.toml"),
        "[processor]\nenabled = [\"spellcheck\"]\n"
    ).unwrap();

    // Build
    let build_output = run_rsb(project_path, &["build"]);
    assert!(build_output.status.success());
    // Checkers no longer create stub files - nothing in out/ for spellcheck

    // Clean is a no-op for checkers (nothing to clean)
    let clean_output = run_rsb(project_path, &["clean", "outputs"]);
    assert!(clean_output.status.success());
}

#[test]
fn spellcheck_stops_after_first_error() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create two markdown files, both with spelling errors.
    // "aaa.md" sorts before "bbb.md", so it will be processed first.
    fs::write(
        project_path.join("aaa.md"),
        "# First\n\nThis has a speling error.\n"
    ).unwrap();

    fs::write(
        project_path.join("bbb.md"),
        "# Second\n\nThis has a diferent error.\n"
    ).unwrap();

    fs::write(
        project_path.join("rsb.toml"),
        "[processor]\nenabled = [\"spellcheck\"]\n"
    ).unwrap();

    let output = run_rsb_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(!output.status.success(),
        "Build should fail with misspelled words");

    let combined = format!("{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr));

    // Only the first file (aaa.md) should have been checked
    assert!(combined.contains("speling"),
        "Should report the error from the first file (aaa.md): {}", combined);
    assert!(!combined.contains("diferent"),
        "Should NOT report the error from the second file (bbb.md) — \
         rsb stops after the first failure: {}", combined);
}

#[test]
fn spellcheck_ignores_code_blocks() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create a markdown file with misspelled words inside code blocks — should pass
    fs::write(
        project_path.join("README.md"),
        "# Hello\n\nThis is correct.\n\n```\nxyzqwert notaword\n```\n\nAlso `inlinecode` is fine.\n"
    ).unwrap();

    fs::write(
        project_path.join("rsb.toml"),
        "[processor]\nenabled = [\"spellcheck\"]\n"
    ).unwrap();

    let output = run_rsb_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(),
        "Build should succeed — code blocks should be ignored: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr));
}
