use crate::common::run_rsconstruct_with_env;
use std::fs;
use tempfile::TempDir;

fn project() -> TempDir {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let p = temp_dir.path();
    fs::write(
        p.join("rsconstruct.toml"),
        "[processor.checker.marp_images]\nsrc_dirs = [\"slides\"]\n",
    )
    .unwrap();
    fs::create_dir_all(p.join("slides/img")).unwrap();
    fs::write(
        p.join("slides/deck.md"),
        "# Deck\n\n![a](img/a.png)\n![remote](https://example.com/x.png)\n",
    )
    .unwrap();
    fs::write(p.join("slides/img/a.png"), "png").unwrap();
    temp_dir
}

#[test]
fn marp_images_passes_when_images_exist_and_fails_when_one_is_missing() {
    let temp_dir = project();
    let p = temp_dir.path();
    let ok = run_rsconstruct_with_env(p, &["build"], &[("NO_COLOR", "1")]);
    assert!(
        ok.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&ok.stdout),
        String::from_utf8_lossy(&ok.stderr)
    );

    fs::write(
        p.join("slides/deck.md"),
        "# Deck\n\n![a](img/a.png)\n![b](img/b.png)\n",
    )
    .unwrap();
    let bad = run_rsconstruct_with_env(p, &["build"], &[("NO_COLOR", "1")]);
    assert!(!bad.status.success());
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&bad.stdout),
        String::from_utf8_lossy(&bad.stderr)
    );
    assert!(text.contains("missing image: img/b.png"), "{text}");
}

/// The verdict depends on the referenced images, so they are inputs:
/// deleting one must run the check again, not reuse the cached pass.
#[test]
fn marp_images_rechecks_when_a_referenced_image_is_deleted() {
    let temp_dir = project();
    let p = temp_dir.path();
    let first = run_rsconstruct_with_env(p, &["build"], &[("NO_COLOR", "1")]);
    assert!(first.status.success());

    fs::remove_file(p.join("slides/img/a.png")).unwrap();
    let second = run_rsconstruct_with_env(p, &["build"], &[("NO_COLOR", "1")]);
    assert!(
        !second.status.success(),
        "the markdown file is unchanged, but its image is gone: stdout={}",
        String::from_utf8_lossy(&second.stdout)
    );
}
