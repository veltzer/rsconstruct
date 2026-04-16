use std::fs;
use crate::common::{setup_test_project, run_rsconstruct, run_rsconstruct_with_env};

#[test]
fn zspell_correct_spelling() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create a markdown file with correct spelling
    fs::write(
        project_path.join("README.md"),
        "# Hello World\n\nThis is a simple document with correct spelling.\n"
    ).unwrap();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.zspell]\nsrc_dirs = [\".\"]\n"
    ).unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build", "-v"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(),
        "Build should succeed with correct spelling: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr));

    // Checkers no longer create stub files - success is recorded in cache database
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Processing:"),
        "Should process zspell: {}", stdout);
}

#[test]
fn zspell_misspelled_word() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create a markdown file with a misspelled word
    fs::write(
        project_path.join("README.md"),
        "# Hello World\n\nThis document has a speling error.\n"
    ).unwrap();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.zspell]\nsrc_dirs = [\".\"]\n"
    ).unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(!output.status.success(),
        "Build should fail with misspelled word");

    let combined = format!("{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr));
    assert!(combined.contains("speling"),
        "Error should mention the misspelled word: {}", combined);
}

#[test]
fn zspell_custom_words_file() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create a markdown file with a "misspelled" word that is actually a custom word
    fs::write(
        project_path.join("README.md"),
        "# Hello World\n\nThis uses rsconstruct for building.\n"
    ).unwrap();

    // Add "rsconstruct" to custom words file
    fs::write(
        project_path.join(".zspell-words"),
        "# Custom project words\nrsconstruct\n"
    ).unwrap();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.zspell]\nsrc_dirs = [\".\"]\n"
    ).unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(),
        "Build should succeed with custom words: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr));
}

#[test]
fn zspell_incremental_skip() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("README.md"),
        "# Hello World\n\nThis is correct.\n"
    ).unwrap();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.zspell]\nsrc_dirs = [\".\"]\n"
    ).unwrap();

    // First build
    let output1 = run_rsconstruct_with_env(project_path, &["build", "-v"], &[("NO_COLOR", "1")]);
    assert!(output1.status.success());
    let stdout1 = String::from_utf8_lossy(&output1.stdout);
    assert!(stdout1.contains("Processing:"),
        "First build should process: {}", stdout1);

    // Second build should skip
    let output2 = run_rsconstruct_with_env(project_path, &["build", "--verbose"], &[("NO_COLOR", "1")]);
    assert!(output2.status.success());
    let stdout2 = String::from_utf8_lossy(&output2.stdout);
    assert!(stdout2.contains("[zspell] Skipping (unchanged):"),
        "Second build should skip: {}", stdout2);
}

#[test]
fn zspell_clean() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("README.md"),
        "# Hello World\n\nThis is correct.\n"
    ).unwrap();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.zspell]\nsrc_dirs = [\".\"]\n"
    ).unwrap();

    // Build
    let build_output = run_rsconstruct(project_path, &["build"]);
    assert!(build_output.status.success());
    // Checkers no longer create stub files - nothing in out/ for zspell

    // Clean is a no-op for checkers (nothing to clean)
    let clean_output = run_rsconstruct(project_path, &["clean", "outputs"]);
    assert!(clean_output.status.success());
}

#[test]
fn zspell_stops_after_first_error() {
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
        project_path.join("rsconstruct.toml"),
        "[processor.zspell]\nsrc_dirs = [\".\"]\n"
    ).unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build", "-j", "1"], &[("NO_COLOR", "1")]);
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
         rsconstruct stops after the first failure: {}", combined);
}

#[test]
fn zspell_ignores_code_blocks() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create a markdown file with misspelled words inside code blocks — should pass
    fs::write(
        project_path.join("README.md"),
        "# Hello\n\nThis is correct.\n\n```\nxyzqwert notaword\n```\n\nAlso `inlinecode` is fine.\n"
    ).unwrap();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.zspell]\nsrc_dirs = [\".\"]\n"
    ).unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(),
        "Build should succeed — code blocks should be ignored: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr));
}

#[test]
fn zspell_auto_add_words() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // Create a markdown file with "misspelled" words (project-specific terms)
    fs::write(
        project_path.join("README.md"),
        "# Hello World\n\nThis uses rsconstruct and tera for building.\n"
    ).unwrap();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.zspell]\nauto_add_words = true\nsrc_dirs = [\".\"]\n"
    ).unwrap();

    // Build should succeed and add words to .zspell-words
    let output = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(),
        "Build should succeed with auto_add_words: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr));

    // Check that the words file was created with the misspelled words
    let words_path = project_path.join(".zspell-words");
    assert!(words_path.exists(), "Words file should be created");

    let words_content = fs::read_to_string(&words_path).unwrap();
    assert!(words_content.contains("rsconstruct"), "Should contain 'rsconstruct': {}", words_content);
    assert!(words_content.contains("tera"), "Should contain 'tera': {}", words_content);

    // Verify output mentions adding words
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Added") && stdout.contains("word"),
        "Should mention adding words: {}", stdout);
}
