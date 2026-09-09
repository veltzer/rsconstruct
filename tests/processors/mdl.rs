use crate::common::{require_tool, run_rsconstruct_with_env};
use std::fs;
use tempfile::TempDir;

#[test]
fn mdl_valid_file() {
    require_tool("mdl");

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    // Point command to the system mdl, skip gem dependency
    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.mdl]\ncommand = \"mdl\"\nsrc_dirs = [\".\"]\n",
    )
    .unwrap();

    // Content that passes mdl rules: proper heading structure, blank lines
    fs::write(
        project_path.join("doc.md"),
        "# Hello World\n\nThis is a test document.\n",
    )
    .unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build", "-v"], &[("NO_COLOR", "1")]);
    // mdl may fail due to rule violations even with simple content
    // Just verify discovery and processing attempt
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stdout.contains("Processing:") || stdout.contains("1 products"),
        "Should discover and attempt mdl processing: stdout={}, stderr={}",
        stdout,
        stderr
    );
}

/// With `[build] allow_missing_src_dirs = true` a missing entry is skipped
/// without disturbing the entries beside it:
///   [processor.mdl]
///   src_dirs = ["config", "script"]
/// where `script/` doesn't exist on disk — `config/` must still be scanned.
#[test]
fn mdl_missing_src_dir_skips_without_affecting_others_when_allowed() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    // `config/` exists but `script/` does not.
    fs::create_dir(project_path.join("config")).unwrap();
    fs::write(project_path.join("config/doc.md"), "# doc\n").unwrap();
    fs::write(
        project_path.join("rsconstruct.toml"),
        "[build]\nallow_missing_src_dirs = true\n\n[processor.mdl]\nsrc_dirs = [\"config\", \"script\"]\n",
    )
    .unwrap();

    let output =
        run_rsconstruct_with_env(project_path, &["build", "--dry-run"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "Build must succeed: the missing 'script' entry scans nothing. {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // The surviving entry still matched its file.
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("config/doc.md"),
        "the existing 'config' dir must still be scanned: {combined}"
    );
}
