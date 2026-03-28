use std::fs;
use crate::common::run_rsconstruct_with_env;
use tempfile::TempDir;

/// Helper: create a tags test project with given .md files and optional .tags file.
fn setup_tags_project(md_files: &[(&str, &str)], tags_file: Option<&str>) -> TempDir {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let p = temp_dir.path();

    let config = "[processor.tags]\n";
    fs::write(p.join("rsconstruct.toml"), config).unwrap();

    for (name, content) in md_files {
        if let Some(parent) = std::path::Path::new(name).parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(p.join(parent)).unwrap();
            }
        }
        fs::write(p.join(name), content).unwrap();
    }

    if let Some(tags_content) = tags_file {
        fs::write(p.join(".tags"), tags_content).unwrap();
    }

    temp_dir
}

/// Helper: build the project and assert success.
fn build_project(p: &std::path::Path) {
    let output = run_rsconstruct_with_env(p, &["build"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(), "build failed: {}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn tags_basic_build_and_query() {
    let temp_dir = setup_tags_project(
        &[
            ("course1.md", "---\nlevel: beginner\ntags:\n  - python\n  - docker\n---\n# Course 1\n"),
            ("course2.md", "---\nlevel: advanced\ntags:\n  - rust\n  - docker\n---\n# Course 2\n"),
        ],
        None,
    );
    let p = temp_dir.path();

    // Build should succeed and create the tags database
    build_project(p);
    assert!(p.join("out/tags/tags.db").exists(), "tags database should be created");

    // `rsconstruct tags list` should show all tags sorted
    let list_output = run_rsconstruct_with_env(p, &["tags", "list"], &[("NO_COLOR", "1")]);
    assert!(list_output.status.success());
    let stdout = String::from_utf8_lossy(&list_output.stdout);
    let tags: Vec<&str> = stdout.lines().collect();
    assert!(tags.contains(&"docker"));
    assert!(tags.contains(&"python"));
    assert!(tags.contains(&"rust"));
    assert!(tags.contains(&"level:beginner"));
    assert!(tags.contains(&"level:advanced"));

    // `rsconstruct tags files docker` should return both files
    let files_output = run_rsconstruct_with_env(p, &["tags", "files", "docker"], &[("NO_COLOR", "1")]);
    assert!(files_output.status.success());
    let files_stdout = String::from_utf8_lossy(&files_output.stdout);
    assert!(files_stdout.contains("course1.md"));
    assert!(files_stdout.contains("course2.md"));

    // `rsconstruct tags files docker rust` (AND) should return only course2
    let and_output = run_rsconstruct_with_env(p, &["tags", "files", "docker", "rust"], &[("NO_COLOR", "1")]);
    assert!(and_output.status.success());
    let and_stdout = String::from_utf8_lossy(&and_output.stdout);
    assert!(!and_stdout.contains("course1.md"));
    assert!(and_stdout.contains("course2.md"));

    // `rsconstruct tags files --or python rust` (OR) should return both files
    let or_output = run_rsconstruct_with_env(p, &["tags", "files", "--or", "python", "rust"], &[("NO_COLOR", "1")]);
    assert!(or_output.status.success());
    let or_stdout = String::from_utf8_lossy(&or_output.stdout);
    assert!(or_stdout.contains("course1.md"));
    assert!(or_stdout.contains("course2.md"));
}

#[test]
fn tags_validation_rejects_unknown_tags() {
    let temp_dir = setup_tags_project(
        &[
            ("course.md", "---\nlevel: beginner\ntags:\n  - python\n  - dockker\n---\n# Course\n"),
        ],
        Some("python\ndocker\nlevel:beginner\n"),
    );
    let p = temp_dir.path();

    // Build should fail because "dockker" is not in .tags
    let output = run_rsconstruct_with_env(p, &["build"], &[("NO_COLOR", "1")]);
    assert!(!output.status.success(), "build should fail with unknown tag");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("dockker"), "error should mention the unknown tag: {}", stderr);
    // Should suggest "docker" as a typo correction
    assert!(stderr.contains("docker"), "error should suggest 'docker': {}", stderr);
}

#[test]
fn tags_validation_allows_wildcard_patterns() {
    let temp_dir = setup_tags_project(
        &[
            ("course.md", "---\nlevel: beginner\nduration_hours: 5\ntags:\n  - python\n---\n# Course\n"),
        ],
        // Wildcard pattern for duration_hours:*
        Some("python\nlevel:beginner\nduration_hours:*\n"),
    );
    let p = temp_dir.path();

    // Build should succeed because duration_hours:5 matches duration_hours:*
    let output = run_rsconstruct_with_env(p, &["build"], &[("NO_COLOR", "1")]);
    assert!(output.status.success(),
        "build should succeed with wildcard pattern: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr));
}

#[test]
fn tags_for_file_path_matching() {
    let temp_dir = setup_tags_project(
        &[
            ("sub/foo.md", "---\ntags:\n  - alpha\n---\n# Foo\n"),
            ("sub/barfoo.md", "---\ntags:\n  - beta\n---\n# Barfoo\n"),
        ],
        None,
    );
    let p = temp_dir.path();

    build_project(p);

    // Querying for "sub/foo.md" should return alpha, NOT beta
    let for_file = run_rsconstruct_with_env(p, &["tags", "for-file", "sub/foo.md"], &[("NO_COLOR", "1")]);
    assert!(for_file.status.success());
    let stdout = String::from_utf8_lossy(&for_file.stdout);
    assert!(stdout.contains("alpha"), "should find tag 'alpha' for sub/foo.md: {}", stdout);
    assert!(!stdout.contains("beta"), "should NOT match barfoo.md's tag 'beta': {}", stdout);
}

#[test]
fn tags_init_and_unused() {
    let temp_dir = setup_tags_project(
        &[
            ("a.md", "---\ntags:\n  - used\n---\n"),
        ],
        None,
    );
    let p = temp_dir.path();

    // Build first to populate db
    build_project(p);

    // Init should create .tags
    let init = run_rsconstruct_with_env(p, &["tags", "init"], &[("NO_COLOR", "1")]);
    assert!(init.status.success());
    assert!(p.join(".tags").exists(), ".tags file should be created");

    let tags_content = fs::read_to_string(p.join(".tags")).unwrap();
    assert!(tags_content.contains("used"));

    // Add an extra tag that doesn't exist in any file
    fs::write(p.join(".tags"), "used\nobsolete\n").unwrap();

    // `rsconstruct tags unused` should report "obsolete" as unused
    let unused = run_rsconstruct_with_env(p, &["tags", "unused"], &[("NO_COLOR", "1")]);
    assert!(unused.status.success());
    let unused_stdout = String::from_utf8_lossy(&unused.stdout);
    assert!(unused_stdout.contains("obsolete"), "should report 'obsolete' as unused: {}", unused_stdout);
    assert!(!unused_stdout.contains("used\n"), "should not report 'used' as unused");
}

#[test]
fn tags_count_and_tree() {
    let temp_dir = setup_tags_project(
        &[
            ("a.md", "---\nlevel: beginner\ntags:\n  - python\n  - docker\n---\n"),
            ("b.md", "---\nlevel: advanced\ntags:\n  - docker\n---\n"),
        ],
        None,
    );
    let p = temp_dir.path();
    build_project(p);

    // Count should show docker with count 2
    let count = run_rsconstruct_with_env(p, &["tags", "count"], &[("NO_COLOR", "1")]);
    assert!(count.status.success());
    let stdout = String::from_utf8_lossy(&count.stdout);
    assert!(stdout.contains("docker"), "count should list docker: {}", stdout);
    // docker appears in 2 files, should be first (highest count)
    let first_line = stdout.lines().next().unwrap();
    assert!(first_line.contains("docker"), "docker should be first (highest count): {}", first_line);

    // Tree should group level= tags
    let tree = run_rsconstruct_with_env(p, &["tags", "tree"], &[("NO_COLOR", "1")]);
    assert!(tree.status.success());
    let tree_stdout = String::from_utf8_lossy(&tree.stdout);
    assert!(tree_stdout.contains("level="), "tree should show level= group: {}", tree_stdout);
    assert!(tree_stdout.contains("beginner"), "tree should show beginner value: {}", tree_stdout);
    assert!(tree_stdout.contains("advanced"), "tree should show advanced value: {}", tree_stdout);
    assert!(tree_stdout.contains("(bare tags)"), "tree should show bare tags section: {}", tree_stdout);
}

#[test]
fn tags_stats() {
    let temp_dir = setup_tags_project(
        &[
            ("a.md", "---\nlevel: beginner\ntags:\n  - python\n---\n"),
            ("b.md", "---\ntags:\n  - docker\n---\n"),
        ],
        None,
    );
    let p = temp_dir.path();
    build_project(p);

    let stats = run_rsconstruct_with_env(p, &["tags", "stats"], &[("NO_COLOR", "1")]);
    assert!(stats.status.success());
    let stdout = String::from_utf8_lossy(&stats.stdout);
    assert!(stdout.contains("Files indexed:"), "stats should show file count: {}", stdout);
    assert!(stdout.contains("2"), "should index 2 files: {}", stdout);
    assert!(stdout.contains("Unique tags:"), "stats should show unique tags: {}", stdout);
}

#[test]
fn tags_grep() {
    let temp_dir = setup_tags_project(
        &[
            ("a.md", "---\ntags:\n  - python\n  - python-advanced\n  - docker\n---\n"),
        ],
        None,
    );
    let p = temp_dir.path();
    build_project(p);

    // Grep for "python" should match both python tags but not docker
    let grep = run_rsconstruct_with_env(p, &["tags", "grep", "python"], &[("NO_COLOR", "1")]);
    assert!(grep.status.success());
    let stdout = String::from_utf8_lossy(&grep.stdout);
    assert!(stdout.contains("python"), "grep should find 'python': {}", stdout);
    assert!(stdout.contains("python-advanced"), "grep should find 'python-advanced': {}", stdout);
    assert!(!stdout.contains("docker"), "grep should NOT find 'docker': {}", stdout);

    // Case-insensitive grep
    let grep_i = run_rsconstruct_with_env(p, &["tags", "grep", "-i", "PYTHON"], &[("NO_COLOR", "1")]);
    assert!(grep_i.status.success());
    let stdout_i = String::from_utf8_lossy(&grep_i.stdout);
    assert!(stdout_i.contains("python"), "case-insensitive grep should find 'python': {}", stdout_i);
}

#[test]
fn tags_frontmatter() {
    let temp_dir = setup_tags_project(
        &[
            ("course.md", "---\ntitle: My Course\nlevel: beginner\ntags:\n  - python\n---\n# Content\n"),
        ],
        None,
    );
    let p = temp_dir.path();
    build_project(p);

    let fm = run_rsconstruct_with_env(p, &["tags", "frontmatter", "course.md"], &[("NO_COLOR", "1")]);
    assert!(fm.status.success());
    let stdout = String::from_utf8_lossy(&fm.stdout);
    assert!(stdout.contains("title"), "frontmatter should show title: {}", stdout);
    assert!(stdout.contains("My Course"), "frontmatter should show title value: {}", stdout);
    assert!(stdout.contains("level"), "frontmatter should show level: {}", stdout);
}

#[test]
fn tags_add_and_remove() {
    let temp_dir = setup_tags_project(
        &[("a.md", "---\ntags:\n  - existing\n---\n")],
        None,
    );
    let p = temp_dir.path();
    build_project(p);

    // Add a new tag
    let add = run_rsconstruct_with_env(p, &["tags", "add", "newtag"], &[("NO_COLOR", "1")]);
    assert!(add.status.success());
    let tags_content = fs::read_to_string(p.join(".tags")).unwrap();
    assert!(tags_content.contains("newtag"), "newtag should be in .tags: {}", tags_content);

    // Add the same tag again — should report already exists
    let add2 = run_rsconstruct_with_env(p, &["tags", "add", "newtag"], &[("NO_COLOR", "1")]);
    assert!(add2.status.success());
    let stdout = String::from_utf8_lossy(&add2.stdout);
    assert!(stdout.contains("already"), "should say tag already exists: {}", stdout);

    // Remove the tag
    let remove = run_rsconstruct_with_env(p, &["tags", "remove", "newtag"], &[("NO_COLOR", "1")]);
    assert!(remove.status.success());
    let tags_content = fs::read_to_string(p.join(".tags")).unwrap();
    assert!(!tags_content.contains("newtag"), "newtag should be removed from .tags: {}", tags_content);
}

#[test]
fn tags_sync_with_prune() {
    let temp_dir = setup_tags_project(
        &[
            ("a.md", "---\ntags:\n  - active\n---\n"),
        ],
        Some("active\nobsolete\n"),
    );
    let p = temp_dir.path();
    build_project(p);

    // Sync without prune — should keep obsolete
    let sync = run_rsconstruct_with_env(p, &["tags", "sync"], &[("NO_COLOR", "1")]);
    assert!(sync.status.success());
    let tags_content = fs::read_to_string(p.join(".tags")).unwrap();
    assert!(tags_content.contains("obsolete"), "sync without prune should keep obsolete: {}", tags_content);

    // Sync with prune — should remove obsolete
    let sync_prune = run_rsconstruct_with_env(p, &["tags", "sync", "--prune"], &[("NO_COLOR", "1")]);
    assert!(sync_prune.status.success());
    let tags_content = fs::read_to_string(p.join(".tags")).unwrap();
    assert!(!tags_content.contains("obsolete"), "sync with prune should remove obsolete: {}", tags_content);
    assert!(tags_content.contains("active"), "sync should keep active: {}", tags_content);
}

#[test]
fn tags_sync_respects_wildcards() {
    let temp_dir = setup_tags_project(
        &[
            ("a.md", "---\ndifficulty: 3\ntags:\n  - python\n---\n"),
        ],
        Some("python\ndifficulty:*\n"),
    );
    let p = temp_dir.path();
    build_project(p);

    // Sync should NOT add difficulty=3 since difficulty=* already covers it
    let sync = run_rsconstruct_with_env(p, &["tags", "sync"], &[("NO_COLOR", "1")]);
    assert!(sync.status.success());
    let tags_content = fs::read_to_string(p.join(".tags")).unwrap();
    assert!(tags_content.contains("difficulty:*"), "wildcard should be preserved: {}", tags_content);
    assert!(!tags_content.contains("difficulty:3"), "concrete value should NOT be added when wildcard exists: {}", tags_content);
}

#[test]
fn tags_inline_yaml_list() {
    let temp_dir = setup_tags_project(
        &[
            ("a.md", "---\ntags: [alpha, beta, gamma]\nlevel: beginner\n---\n# Content\n"),
        ],
        None,
    );
    let p = temp_dir.path();
    build_project(p);

    let list = run_rsconstruct_with_env(p, &["tags", "list"], &[("NO_COLOR", "1")]);
    assert!(list.status.success());
    let stdout = String::from_utf8_lossy(&list.stdout);
    assert!(stdout.contains("alpha"), "should parse inline list item 'alpha': {}", stdout);
    assert!(stdout.contains("beta"), "should parse inline list item 'beta': {}", stdout);
    assert!(stdout.contains("gamma"), "should parse inline list item 'gamma': {}", stdout);
}

#[test]
fn tags_colon_in_yaml_value() {
    let temp_dir = setup_tags_project(
        &[
            ("a.md", "---\nurl: https://example.com/path\ntime: 10:30\ntags:\n  - web\n---\n"),
        ],
        None,
    );
    let p = temp_dir.path();
    build_project(p);

    let fm = run_rsconstruct_with_env(p, &["tags", "frontmatter", "a.md"], &[("NO_COLOR", "1")]);
    assert!(fm.status.success());
    let stdout = String::from_utf8_lossy(&fm.stdout);
    assert!(stdout.contains("https://example.com/path"), "URL value should be preserved: {}", stdout);
    assert!(stdout.contains("10:30"), "time value should be preserved: {}", stdout);

    // Also check tags list for key=value indexing
    let list = run_rsconstruct_with_env(p, &["tags", "list"], &[("NO_COLOR", "1")]);
    assert!(list.status.success());
    let list_stdout = String::from_utf8_lossy(&list.stdout);
    assert!(list_stdout.contains("url:https://example.com/path"), "URL should be indexed correctly: {}", list_stdout);
    assert!(list_stdout.contains("time:10:30"), "time should be indexed correctly: {}", list_stdout);
}

#[test]
fn tags_unused_strict_fails() {
    let temp_dir = setup_tags_project(
        &[
            ("a.md", "---\ntags:\n  - active\n---\n"),
        ],
        Some("active\nobsolete\n"),
    );
    let p = temp_dir.path();
    build_project(p);

    // Without --strict, should succeed even with unused tags
    let unused = run_rsconstruct_with_env(p, &["tags", "unused"], &[("NO_COLOR", "1")]);
    assert!(unused.status.success(), "unused without --strict should succeed");

    // With --strict, should fail
    let unused_strict = run_rsconstruct_with_env(p, &["tags", "unused", "--strict"], &[("NO_COLOR", "1")]);
    assert!(!unused_strict.status.success(), "unused with --strict should fail when unused tags exist");
}

#[test]
fn tags_validate_standalone() {
    // Build without .tags so build succeeds, then add .tags and run validate
    let temp_dir = setup_tags_project(
        &[
            ("a.md", "---\ntags:\n  - python\n  - dockker\n---\n"),
        ],
        None,
    );
    let p = temp_dir.path();
    build_project(p);

    // Now create .tags file with allowed tags (dockker is a typo not in the list)
    fs::write(p.join(".tags"), "python\ndocker\n").unwrap();

    // validate should fail and suggest the correct tag
    let validate = run_rsconstruct_with_env(p, &["tags", "validate"], &[("NO_COLOR", "1")]);
    assert!(!validate.status.success(), "validate should fail with unknown tags");
    let stderr = String::from_utf8_lossy(&validate.stderr);
    assert!(stderr.contains("dockker"), "should mention unknown tag: {}", stderr);
    assert!(stderr.contains("docker"), "should suggest correction: {}", stderr);
}

#[test]
fn tags_numeric_and_boolean_values() {
    let temp_dir = setup_tags_project(
        &[
            ("a.md", "---\ndifficulty: 3\npublished: true\ntags:\n  - test\n---\n"),
        ],
        None,
    );
    let p = temp_dir.path();
    build_project(p);

    let list = run_rsconstruct_with_env(p, &["tags", "list"], &[("NO_COLOR", "1")]);
    assert!(list.status.success());
    let stdout = String::from_utf8_lossy(&list.stdout);
    // Our simple YAML parser returns everything as strings, so these are string values
    assert!(stdout.contains("difficulty:3"), "numeric value should be indexed: {}", stdout);
    assert!(stdout.contains("published:true"), "boolean value should be indexed: {}", stdout);
}

#[test]
fn tags_stale_entries_cleared_on_rebuild() {
    let temp_dir = setup_tags_project(
        &[
            ("a.md", "---\ntags:\n  - alpha\n  - beta\n---\n"),
        ],
        None,
    );
    let p = temp_dir.path();
    build_project(p);

    // Verify both tags exist
    let list1 = run_rsconstruct_with_env(p, &["tags", "list"], &[("NO_COLOR", "1")]);
    let stdout1 = String::from_utf8_lossy(&list1.stdout);
    assert!(stdout1.contains("alpha"));
    assert!(stdout1.contains("beta"));

    // Remove "beta" tag from the file and force rebuild
    fs::write(p.join("a.md"), "---\ntags:\n  - alpha\n---\n").unwrap();
    let rebuild = run_rsconstruct_with_env(p, &["build", "--force"], &[("NO_COLOR", "1")]);
    assert!(rebuild.status.success(), "rebuild failed: {}", String::from_utf8_lossy(&rebuild.stderr));

    // "beta" should no longer appear
    let list2 = run_rsconstruct_with_env(p, &["tags", "list"], &[("NO_COLOR", "1")]);
    let stdout2 = String::from_utf8_lossy(&list2.stdout);
    assert!(stdout2.contains("alpha"), "alpha should still exist: {}", stdout2);
    assert!(!stdout2.contains("beta"), "beta should be gone after rebuild: {}", stdout2);
}

#[test]
fn tags_empty_inline_list() {
    let temp_dir = setup_tags_project(
        &[
            ("a.md", "---\ntags: []\nlevel: beginner\n---\n# Content\n"),
        ],
        None,
    );
    let p = temp_dir.path();
    build_project(p);

    // The empty list should not produce any bare tags (no phantom empty-string tag)
    let list = run_rsconstruct_with_env(p, &["tags", "list"], &[("NO_COLOR", "1")]);
    assert!(list.status.success());
    let stdout = String::from_utf8_lossy(&list.stdout);
    // Should only have level:beginner, no empty tags
    assert!(stdout.contains("level:beginner"), "should have level:beginner: {}", stdout);
    let tags: Vec<&str> = stdout.lines().collect();
    assert_eq!(tags.len(), 1, "should have exactly 1 tag (level:beginner), got: {:?}", tags);
}
