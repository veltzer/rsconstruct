use crate::common::{run_rsconstruct_json_with_env, run_rsconstruct_with_env};
use std::fs;
use tempfile::TempDir;

/// Create a test project with a creator processor that produces two output directories.
/// The script creates dir_a/file_a.txt and dir_b/file_b.txt.
fn setup_creator_project() -> TempDir {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    // Write the creator script
    let script = project_path.join("create.sh");
    fs::write(
        &script,
        concat!(
            "#!/bin/bash\n",
            "set -e\n",
            "mkdir -p dir_a dir_b\n",
            "echo 'content_a' > dir_a/file_a.txt\n",
            "echo 'content_b' > dir_b/file_b.txt\n",
        ),
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    }

    // Write the anchor file (the script scans for this)
    fs::write(project_path.join("trigger.manifest"), "trigger\n").unwrap();

    // Configure the creator processor
    fs::write(
        project_path.join("rsconstruct.toml"),
        concat!(
            "[processor.creator.my_creator]\n",
            "command = \"./create.sh\"\n",
            "src_extensions = [\".manifest\"]\n",
            "src_dirs = [\".\"]\n",
            "output_dirs = [\"dir_a\", \"dir_b\"]\n",
        ),
    )
    .unwrap();

    temp_dir
}

#[test]
fn creator_produces_two_output_dirs() {
    let temp_dir = setup_creator_project();
    let project_path = temp_dir.path();

    let output = run_rsconstruct_with_env(project_path, &["build", "-v"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "Build should succeed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    // Verify both dirs and files were created
    assert!(
        project_path.join("dir_a/file_a.txt").exists(),
        "dir_a/file_a.txt should exist"
    );
    assert!(
        project_path.join("dir_b/file_b.txt").exists(),
        "dir_b/file_b.txt should exist"
    );
    assert_eq!(
        fs::read_to_string(project_path.join("dir_a/file_a.txt"))
            .unwrap()
            .trim(),
        "content_a"
    );
    assert_eq!(
        fs::read_to_string(project_path.join("dir_b/file_b.txt"))
            .unwrap()
            .trim(),
        "content_b"
    );
}

#[test]
fn creator_clean_removes_output_dirs() {
    let temp_dir = setup_creator_project();
    let project_path = temp_dir.path();

    // Build first
    let build = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(build.status.success());
    assert!(project_path.join("dir_a").exists());
    assert!(project_path.join("dir_b").exists());

    // Clean outputs
    let clean = run_rsconstruct_with_env(project_path, &["clean", "outputs"], &[("NO_COLOR", "1")]);
    assert!(
        clean.status.success(),
        "Clean should succeed: stderr={}",
        String::from_utf8_lossy(&clean.stderr),
    );

    // Verify dirs are gone
    assert!(
        !project_path.join("dir_a").exists(),
        "dir_a should be removed after clean"
    );
    assert!(
        !project_path.join("dir_b").exists(),
        "dir_b should be removed after clean"
    );
    // Cache should still exist
    assert!(
        project_path.join(".rsconstruct").exists(),
        "cache should be preserved"
    );
}

#[test]
fn creator_restores_output_dirs_from_cache() {
    let temp_dir = setup_creator_project();
    let project_path = temp_dir.path();

    // Build to populate cache
    let build = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(build.status.success());

    // Clean outputs (but preserve cache)
    let clean = run_rsconstruct_with_env(project_path, &["clean", "outputs"], &[("NO_COLOR", "1")]);
    assert!(clean.status.success());
    assert!(!project_path.join("dir_a").exists());
    assert!(!project_path.join("dir_b").exists());

    // Rebuild — should restore from cache
    let restore =
        run_rsconstruct_with_env(project_path, &["build", "--verbose"], &[("NO_COLOR", "1")]);
    assert!(
        restore.status.success(),
        "Restore build should succeed: stdout={}, stderr={}",
        String::from_utf8_lossy(&restore.stdout),
        String::from_utf8_lossy(&restore.stderr),
    );

    let stdout = String::from_utf8_lossy(&restore.stdout);
    assert!(
        stdout.contains("Restored from cache:"),
        "Should restore from cache: {}",
        stdout,
    );

    // Verify restored content is correct
    assert!(
        project_path.join("dir_a/file_a.txt").exists(),
        "dir_a/file_a.txt should be restored"
    );
    assert!(
        project_path.join("dir_b/file_b.txt").exists(),
        "dir_b/file_b.txt should be restored"
    );
    assert_eq!(
        fs::read_to_string(project_path.join("dir_a/file_a.txt"))
            .unwrap()
            .trim(),
        "content_a"
    );
    assert_eq!(
        fs::read_to_string(project_path.join("dir_b/file_b.txt"))
            .unwrap()
            .trim(),
        "content_b"
    );
}

#[test]
fn creator_incremental_skip() {
    let temp_dir = setup_creator_project();
    let project_path = temp_dir.path();

    // First build
    let build1 = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(build1.status.success());

    // Second build — should skip
    let build2 =
        run_rsconstruct_with_env(project_path, &["build", "--verbose"], &[("NO_COLOR", "1")]);
    assert!(
        build2.status.success(),
        "Second build should succeed: stdout={}, stderr={}",
        String::from_utf8_lossy(&build2.stdout),
        String::from_utf8_lossy(&build2.stderr),
    );
    let stdout = String::from_utf8_lossy(&build2.stdout);
    assert!(
        stdout.contains("Skipping (unchanged):"),
        "Second build should skip: {}",
        stdout,
    );
}

/// A creator whose outputs were hardlink-restored from the cache must still
/// be rebuildable when its input changes.
///
/// Restored outputs share the cache object's inode and are read-only; a tool
/// that opens them for writing in place (sphinx, `cat >`, ...) gets EACCES.
/// The previous build's tree is unlinked before the rebuild — but that tree's
/// descriptor key is no longer derivable once the input changed, so it must
/// be found through the per-product last-tree pointer.
#[test]
fn creator_rebuilds_over_hardlink_restored_outputs() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    let script = project_path.join("create.sh");
    fs::write(
        &script,
        concat!(
            "#!/bin/bash\n",
            "set -e\n",
            "mkdir -p dir_a\n",
            // Open-for-write in place, as sphinx does: fails on a read-only file.
            "cat trigger.manifest > dir_a/file_a.txt\n",
        ),
    )
    .unwrap();
    crate::common::make_executable(&script);
    fs::write(project_path.join("trigger.manifest"), "first\n").unwrap();
    fs::write(
        project_path.join("rsconstruct.toml"),
        concat!(
            // CI resolves `auto` to copy, which never produces read-only outputs.
            "[cache]\n",
            "restore_method = \"hardlink\"\n",
            "[processor.creator.my_creator]\n",
            "command = \"./create.sh\"\n",
            "src_extensions = [\".manifest\"]\n",
            "src_dirs = [\".\"]\n",
            "output_dirs = [\"dir_a\"]\n",
        ),
    )
    .unwrap();
    let env = [("NO_COLOR", "1")];
    let file_a = project_path.join("dir_a/file_a.txt");

    // Two cycles: the second proves the pointer follows the *new* tree after
    // a rebuild, not just the first one ever recorded.
    for (round, (old, new)) in [("first", "second"), ("second", "third")]
        .into_iter()
        .enumerate()
    {
        let build = run_rsconstruct_json_with_env(project_path, &["build"], &env);
        assert!(
            build.exit_success,
            "round {round}: build: {:?}",
            build.errors
        );
        assert_eq!(fs::read_to_string(&file_a).unwrap().trim(), old);

        let clean = run_rsconstruct_with_env(project_path, &["clean", "outputs"], &env);
        assert!(clean.status.success());
        let restore = run_rsconstruct_json_with_env(project_path, &["build"], &env);
        assert!(
            restore.exit_success,
            "round {round}: restore: {:?}",
            restore.errors
        );
        assert_eq!(
            restore.restored, 1,
            "round {round}: expected a cache restore"
        );
        assert!(
            fs::metadata(&file_a).unwrap().permissions().readonly(),
            "round {round}: a hardlink restore leaves the output read-only"
        );

        fs::write(project_path.join("trigger.manifest"), format!("{new}\n")).unwrap();
        let rebuild = run_rsconstruct_json_with_env(project_path, &["build"], &env);
        assert!(
            rebuild.exit_success,
            "round {round}: rebuild over restored outputs failed: {:?}",
            rebuild.errors
        );
        assert_eq!(
            rebuild.success, 1,
            "round {round}: expected one rebuilt product"
        );
        assert_eq!(fs::read_to_string(&file_a).unwrap().trim(), new);
    }
}
