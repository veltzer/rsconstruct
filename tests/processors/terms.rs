use std::fs;
use crate::common::{setup_test_project, run_rsconstruct_with_env};

fn write_terms_dirs(project_path: &std::path::Path, single: &[&str], ambiguous: &[&str]) {
    let single_dir = project_path.join("terms.single_meaning");
    let amb_dir = project_path.join("terms.ambiguous");
    fs::create_dir_all(&single_dir).unwrap();
    fs::create_dir_all(&amb_dir).unwrap();
    fs::write(single_dir.join("words.txt"), single.join("\n") + "\n").unwrap();
    fs::write(amb_dir.join("words.txt"), ambiguous.join("\n") + "\n").unwrap();
}

#[test]
fn terms_two_dirs_disjoint_passes() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    write_terms_dirs(project_path, &["Kubernetes", "Docker"], &["server", "client"]);

    fs::write(
        project_path.join("README.md"),
        "# Doc\n\nWe deploy on `Kubernetes` with `Docker`.\n",
    ).unwrap();

    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.terms]\nterms_dir = \"terms.single_meaning\"\nambiguous_terms_dir = \"terms.ambiguous\"\nsrc_dirs = [\".\"]\n",
    ).unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "Build should succeed with disjoint dirs: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn terms_two_dirs_overlap_fails() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // "Docker" appears in both -> must fail
    write_terms_dirs(project_path, &["Kubernetes", "Docker"], &["Docker", "client"]);

    fs::write(project_path.join("README.md"), "# Doc\n").unwrap();
    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.terms]\nterms_dir = \"terms.single_meaning\"\nambiguous_terms_dir = \"terms.ambiguous\"\nsrc_dirs = [\".\"]\n",
    ).unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(!output.status.success(), "Build should fail when terms overlap");

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("Docker") && combined.contains("ambiguous"),
        "Error should mention overlapping term and ambiguous: {}",
        combined
    );
}

#[test]
fn terms_ambiguous_terms_are_not_flagged() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    write_terms_dirs(project_path, &["Kubernetes"], &["server"]);

    // "server" is ambiguous - must NOT be required to be backticked.
    // "Kubernetes" is single-meaning and IS backticked - should pass.
    fs::write(
        project_path.join("README.md"),
        "# Doc\n\nThe server runs on `Kubernetes`.\n",
    ).unwrap();
    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.terms]\nterms_dir = \"terms.single_meaning\"\nambiguous_terms_dir = \"terms.ambiguous\"\nsrc_dirs = [\".\"]\n",
    ).unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "Ambiguous terms should not be required to be backticked: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
