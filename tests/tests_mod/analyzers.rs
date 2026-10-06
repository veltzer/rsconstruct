use crate::common::run_rsconstruct_with_env;
use std::fs;
use tempfile::TempDir;

/// `enabled = false` on an analyzer stanza must keep it out of the active set —
/// `analyzer used` is the public surface for this and should omit disabled analyzers.
#[test]
fn analyzer_disabled_via_enabled_false() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        r#"[processor.generator.markdown2html]
src_dirs = ["."]

[analyzer.markdown]
enabled = false
"#,
    )
    .unwrap();

    fs::write(project_path.join("doc.md"), "# hi\n").unwrap();

    let output =
        run_rsconstruct_with_env(project_path, &["analyzer", "used"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("markdown"),
        "Disabled analyzer should not appear in `analyzer used`: {}",
        stdout
    );
}

/// `enabled = true` (the default) keeps the analyzer active — sanity check that
/// the toggle isn't stuck off.
#[test]
fn analyzer_enabled_true_is_active() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        r#"[processor.generator.markdown2html]
src_dirs = ["."]

[analyzer.markdown]
enabled = true
"#,
    )
    .unwrap();

    fs::write(project_path.join("doc.md"), "# hi\n").unwrap();

    let output =
        run_rsconstruct_with_env(project_path, &["analyzer", "used"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("markdown"),
        "Enabled analyzer should appear in `analyzer used`: {}",
        stdout
    );
}

/// Unknown analyzer type must produce a schema error at config-load time,
/// before anything else runs. `toml check` should surface the error.
#[test]
fn analyzer_unknown_type_is_config_error() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        r#"[processor.generator.markdown2html]
src_dirs = ["."]

[analyzer.not_a_real_analyzer]
"#,
    )
    .unwrap();

    let output = run_rsconstruct_with_env(project_path, &["toml", "check"], &[("NO_COLOR", "1")]);
    assert!(
        !output.status.success(),
        "Config with unknown analyzer must fail validation"
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("not_a_real_analyzer") && combined.contains("unknown analyzer"),
        "Error should name the unknown analyzer: {}",
        combined
    );
}

/// Unknown field in a known analyzer must produce a schema error listing the
/// valid fields, so the user can spot the typo.
#[test]
fn analyzer_unknown_field_is_config_error() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        r#"[processor.generator.markdown2html]
src_dirs = ["."]

[analyzer.markdown]
enabeld = false
"#,
    )
    .unwrap();

    let output = run_rsconstruct_with_env(project_path, &["toml", "check"], &[("NO_COLOR", "1")]);
    assert!(
        !output.status.success(),
        "Config with unknown analyzer field must fail validation"
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("enabeld") && combined.contains("unknown field"),
        "Error should name the typo field: {}",
        combined
    );
    assert!(
        combined.contains("enabled"),
        "Error should list valid fields to help fix the typo: {}",
        combined
    );
}

/// Omitting `enabled` entirely must default to true (backward-compatible with
/// existing rsconstruct.toml files that predate the field).
#[test]
fn analyzer_enabled_defaults_to_true() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        r#"[processor.generator.markdown2html]
src_dirs = ["."]

[analyzer.markdown]
"#,
    )
    .unwrap();

    fs::write(project_path.join("doc.md"), "# hi\n").unwrap();

    let output =
        run_rsconstruct_with_env(project_path, &["analyzer", "used"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("markdown"),
        "Analyzer with no `enabled` field should default to active: {}",
        stdout
    );
}

/// The dependency scanner must avoid re-reading unchanged source files.
/// After a first build populates the deps cache, the second build should
/// report every file as a cache hit (0 rescanned). This exercises the mtime
/// short-circuit in `checksum_fast` together with the content-checksum
/// comparison in `DepsCache::get`.
#[test]
fn analyzer_deps_cache_reports_hits_on_unchanged_files() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        r#"[processor.generator.markdown2html]
src_dirs = ["."]

[analyzer.markdown]
"#,
    )
    .unwrap();

    // Three markdown files with image refs so the analyzer has real work.
    for i in 1..=3 {
        fs::write(
            project_path.join(format!("doc{i}.md")),
            format!("# Doc {i}\n![img](pic{i}.png)\n"),
        )
        .unwrap();
        fs::write(project_path.join(format!("pic{i}.png")), []).unwrap();
    }

    // First build: populates deps cache + mtime cache. We don't assert the
    // first-run hit/miss ratio because `DepsCache::get` has a pre-existing
    // quirk where the very first call against a fresh DB doesn't register
    // as a miss (returns None without incrementing the counter).
    let out1 = run_rsconstruct_with_env(project_path, &["status"], &[("NO_COLOR", "1")]);
    assert!(
        out1.status.success(),
        "first status failed: {}",
        String::from_utf8_lossy(&out1.stderr)
    );

    // Second run with unchanged files: every file should hit the cache.
    let out2 = run_rsconstruct_with_env(project_path, &["status"], &[("NO_COLOR", "1")]);
    assert!(
        out2.status.success(),
        "second status failed: {}",
        String::from_utf8_lossy(&out2.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out2.stdout),
        String::from_utf8_lossy(&out2.stderr)
    );
    assert!(
        combined.contains("[deps] 3 files to check")
            && combined.contains("[deps] summary: 0 rescanned (3 cache hits:"),
        "unchanged files should all hit the cache: {}",
        combined
    );
}

/// Modifying a source file must invalidate its deps cache entry. The mtime
/// change is what drives the invalidation end-to-end: `checksum_fast` sees
/// a new mtime, recomputes the checksum, and `DepsCache::get` sees the
/// mismatch and treats it as a miss.
#[test]
fn analyzer_deps_cache_rescans_changed_file() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        r#"[processor.generator.markdown2html]
src_dirs = ["."]

[analyzer.markdown]
"#,
    )
    .unwrap();

    for i in 1..=3 {
        fs::write(
            project_path.join(format!("doc{i}.md")),
            format!("# Doc {i}\n![img](pic{i}.png)\n"),
        )
        .unwrap();
        fs::write(project_path.join(format!("pic{i}.png")), []).unwrap();
    }

    // Prime the cache.
    let _ = run_rsconstruct_with_env(project_path, &["status"], &[("NO_COLOR", "1")]);

    // Wait for mtime granularity, then modify one file.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    fs::write(
        project_path.join("doc1.md"),
        "# Doc 1 (modified)\n![img](pic1.png)\n",
    )
    .unwrap();

    let out = run_rsconstruct_with_env(project_path, &["status"], &[("NO_COLOR", "1")]);
    assert!(out.status.success());
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        combined.contains("[deps] 3 files to check")
            && combined.contains("[deps] summary: 1 rescanned (2 cache hits:"),
        "modified file should trigger exactly one rescan: {}",
        combined
    );
}

/// When several processors all consume the same source file, an analyzer must
/// scan that file ONCE — not once per consuming product. The pre-scan classify
/// pass and the actual scan both used to iterate `(analyzer, source)` per
/// product, doing redundant cache lookups and (on a miss) redundant file reads
/// for the same source. The display reports unique sources with the per-product
/// fan-out shown in parentheses, and the underlying cache `get`/`set` calls
/// (visible in the summary line) must equal the number of UNIQUE source files.
#[test]
fn analyzer_dedupes_shared_source_across_processors() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    // Three processors, all consuming the same single .md file. The markdown
    // analyzer would naively be invoked 3 times — once per product.
    fs::write(
        project_path.join("rsconstruct.toml"),
        r#"[processor.generator.markdown2html]
src_dirs = ["."]

[processor.checker.zspell]
src_dirs = ["."]

[processor.checker.markdownlint]
src_dirs = ["."]

[analyzer.markdown]
"#,
    )
    .unwrap();

    fs::write(project_path.join("doc.md"), "# hi\n![pic](pic.png)\n").unwrap();
    fs::write(project_path.join("pic.png"), []).unwrap();

    // Prime the cache.
    let _ = run_rsconstruct_with_env(project_path, &["status"], &[("NO_COLOR", "1")]);

    // Second run: display shows 1 unique file with fan-out (consumed by 3
    // products), and the cache sees exactly 1 hit — the source was scanned
    // once and fanned out to all 3 products.
    let out = run_rsconstruct_with_env(project_path, &["status"], &[("NO_COLOR", "1")]);
    assert!(
        out.status.success(),
        "second status failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        combined.contains("[deps] 1 files to check (consumed by 3 products)"),
        "display should show unique-source count with per-product fan-out: {}",
        combined
    );
    assert!(
        combined.contains("[deps] summary: 0 rescanned (1 cache hits:"),
        "shared source must be looked up in the cache exactly once, \
         not once per consuming product: {}",
        combined
    );
}

/// Write a file under the project, creating its directory.
fn write_project_file(project_path: &std::path::Path, rel: &str, content: &str) {
    let path = project_path.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

/// Run `rsconstruct build`, failing the test on a non-zero exit.
fn build_ok(project_path: &std::path::Path) {
    let out = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(
        out.status.success(),
        "build failed: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

const ISASS_WITH_SASS_ANALYZER: &str = r#"[processor.generator.isass]
src_dirs = ["sass"]

[analyzer.sass]
"#;

/// Editing a partial must rebuild every stylesheet that `@use`s it. Without
/// the sass analyzer the importer's only input was its own file, so the
/// build called it unchanged and kept serving the old CSS.
#[test]
fn sass_analyzer_rebuilds_importer_when_partial_changes() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();
    write_project_file(project_path, "rsconstruct.toml", ISASS_WITH_SASS_ANALYZER);
    write_project_file(project_path, "sass/_vars.scss", "$c: red;\n");
    write_project_file(
        project_path,
        "sass/style.scss",
        "@use \"vars\";\nbody { color: vars.$c; }\n",
    );
    build_ok(project_path);

    std::thread::sleep(std::time::Duration::from_millis(1100));
    write_project_file(project_path, "sass/_vars.scss", "$c: blue;\n");
    build_ok(project_path);

    let css =
        fs::read_to_string(project_path.join("out/processor.generator.isass/style.css")).unwrap();
    assert!(css.contains("blue"), "style.css is stale: {css}");
}

/// A `@use` added inside a partial (the importer itself unchanged) must be
/// picked up too: the analyzer rescans instead of trusting a dependency list
/// cached against the importer's own checksum.
#[test]
fn sass_analyzer_follows_use_added_inside_partial() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();
    write_project_file(project_path, "rsconstruct.toml", ISASS_WITH_SASS_ANALYZER);
    write_project_file(project_path, "sass/_vars.scss", "$c: red;\n");
    write_project_file(
        project_path,
        "sass/style.scss",
        "@use \"vars\";\nbody { color: vars.$c; }\n",
    );
    build_ok(project_path);

    std::thread::sleep(std::time::Duration::from_millis(1100));
    write_project_file(project_path, "sass/_more.scss", "$x: green;\n");
    write_project_file(
        project_path,
        "sass/_vars.scss",
        "@use \"more\";\n$c: more.$x;\n",
    );
    build_ok(project_path);

    std::thread::sleep(std::time::Duration::from_millis(1100));
    write_project_file(project_path, "sass/_more.scss", "$x: purple;\n");
    build_ok(project_path);

    let css =
        fs::read_to_string(project_path.join("out/processor.generator.isass/style.css")).unwrap();
    assert!(css.contains("purple"), "style.css is stale: {css}");
}

/// An import that resolves nowhere is an analyzer error naming the file and
/// the URL, unless `skip_not_found` is set.
#[test]
fn sass_analyzer_rejects_unresolved_import() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();
    write_project_file(project_path, "rsconstruct.toml", ISASS_WITH_SASS_ANALYZER);
    write_project_file(project_path, "sass/style.scss", "@use \"nope\";\n");

    let out = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(!out.status.success(), "build should fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Sass import not found: \"nope\" in sass/style.scss"),
        "unexpected error: {stderr}"
    );
}
