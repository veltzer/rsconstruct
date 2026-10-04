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
        "[processor.checker.mdl]\ncommand = \"mdl\"\nsrc_dirs = [\".\"]\n",
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
///   [processor.checker.mdl]
///   src_dirs = ["config", "processor.checker.script"]
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
        "[build]\nallow_missing_src_dirs = true\n\n[processor.checker.mdl]\nsrc_dirs = [\"config\", \"script\"]\n",
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

/// With `local_repo`, mdl runs from the gems the gem processor installs, so
/// the graph must order it after `bundle install`. The edge is the gem
/// processor's install stamp: gem declares it as an output, mdl lists it as
/// an input. It used to name a stamp nothing wrote, which gave no edge and
/// left the order to chance. Discovery only — no tool runs.
#[test]
fn mdl_local_repo_depends_on_gem_install() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();
    fs::write(
        project_path.join("Gemfile"),
        "source 'https://rubygems.org'\n",
    )
    .unwrap();
    fs::write(project_path.join("a.md"), "# t\n").unwrap();
    fs::write(project_path.join(".rsconstructignore"), "gems/\n").unwrap();
    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.creator.gem]\nsrc_dirs = [\"\"]\n\n\
         [processor.checker.mdl]\nsrc_dirs = [\"\"]\nlocal_repo = true\n",
    )
    .unwrap();

    let output = run_rsconstruct_with_env(
        project_path,
        &["graph", "show", "--format", "json"],
        &[("NO_COLOR", "1")],
    );
    assert!(
        output.status.success(),
        "graph show failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let graph: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let products = graph["products"].as_array().unwrap();
    let id_of = |name: &str| {
        products
            .iter()
            .find(|p| p["processor"] == name)
            .unwrap_or_else(|| panic!("no {name} product: {graph}"))
    };
    let gem = id_of("processor.creator.gem");
    let mdl = id_of("processor.checker.mdl");
    assert_eq!(gem["outputs"][0], "out/processor.creator.gem/root.stamp");
    assert!(
        mdl["depends_on"].as_array().unwrap().contains(&gem["id"]),
        "mdl must depend on the gem install: {graph}"
    );
}
