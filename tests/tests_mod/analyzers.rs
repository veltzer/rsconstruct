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

/// When a header starts including another header, the sources that include
/// it must be rescanned so the new header becomes their input too. The
/// cached list used to be validated against the source's own checksum only;
/// `main.c` is unchanged, so its list stayed `[a.h]` and later edits to
/// `b.h` never rebuilt it. `encoding` stands in for a compiler: any product
/// whose primary input is a `.c` file gets the icpp dependencies.
#[test]
fn icpp_rescans_source_when_included_header_gains_include() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();
    write_project_file(
        project_path,
        "rsconstruct.toml",
        "[processor.checker.encoding]\nsrc_dirs = [\"src\"]\nsrc_extensions = [\".c\"]\n\n[analyzer.icpp]\n",
    );
    write_project_file(project_path, "src/main.c", "#include \"a.h\"\n");
    write_project_file(project_path, "src/a.h", "#define V 1\n");
    build_ok(project_path);

    std::thread::sleep(std::time::Duration::from_millis(1100));
    write_project_file(project_path, "src/a.h", "#include \"b.h\"\n");
    write_project_file(project_path, "src/b.h", "#define V 2\n");
    build_ok(project_path);

    let out = run_rsconstruct_with_env(
        project_path,
        &["analyzer", "show", "files", "src/main.c"],
        &[("NO_COLOR", "1")],
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("src/b.h"),
        "main.c should now depend on b.h through a.h: {stdout}"
    );
}

/// `encoding` stands in for any processor of `.py` files: every product
/// whose primary input is a `.py` file gets the python analyzer's imports.
const ENCODING_WITH_PYTHON_ANALYZER: &str = "[processor.checker.encoding]\nsrc_dirs = [\"src\"]\nsrc_extensions = [\".py\"]\n\n[analyzer.python]\n";

fn analyzer_files(project_path: &std::path::Path, source: &str) -> String {
    let out = run_rsconstruct_with_env(
        project_path,
        &["analyzer", "show", "files", source],
        &[("NO_COLOR", "1")],
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Running `main.py` runs what its imports import, so a module two imports
/// away is a dependency: editing it must rebuild `main.py`'s product. The
/// analyzer used to record direct imports only.
#[test]
fn python_analyzer_follows_imports_transitively() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();
    write_project_file(
        project_path,
        "rsconstruct.toml",
        ENCODING_WITH_PYTHON_ANALYZER,
    );
    write_project_file(project_path, "src/main.py", "import os\nimport helper\n");
    write_project_file(project_path, "src/helper.py", "from util import x\n");
    write_project_file(project_path, "src/util.py", "x = 1\n");
    build_ok(project_path);

    let files = analyzer_files(project_path, "src/main.py");
    assert!(files.contains("src/helper.py"), "direct import: {files}");
    assert!(
        files.contains("src/util.py"),
        "import of an import: {files}"
    );

    std::thread::sleep(std::time::Duration::from_millis(1100));
    write_project_file(project_path, "src/util.py", "x = 2\n");
    let out = run_rsconstruct_with_env(project_path, &["build", "--dry-run"], &[("NO_COLOR", "1")]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("BUILD [processor.checker.encoding] src/main.py"),
        "main.py must rebuild when util.py changes: {stdout}"
    );
}

/// A `src/`-layout package imported by absolute name: `from app.util import
/// fs` resolves under `src/` (the default `search_paths`), picks up the
/// submodule `fs`, and depends on every `__init__.py` that importing it runs.
#[test]
fn python_analyzer_resolves_src_layout_packages() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();
    write_project_file(
        project_path,
        "rsconstruct.toml",
        ENCODING_WITH_PYTHON_ANALYZER,
    );
    write_project_file(project_path, "src/app/__init__.py", "");
    write_project_file(
        project_path,
        "src/app/main.py",
        "from app.util import fs, VERSION\n",
    );
    write_project_file(project_path, "src/app/util/__init__.py", "VERSION = 1\n");
    write_project_file(project_path, "src/app/util/fs.py", "import os, app.log\n");
    write_project_file(project_path, "src/app/log.py", "");
    build_ok(project_path);

    let files = analyzer_files(project_path, "src/app/main.py");
    for dep in [
        "src/app/__init__.py",
        "src/app/util/__init__.py",
        "src/app/util/fs.py",
        "src/app/log.py",
    ] {
        assert!(files.contains(dep), "missing {dep}: {files}");
    }
}

/// Modules that import each other terminate the scan, and a module is not
/// its own dependency.
#[test]
fn python_analyzer_handles_import_cycles() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();
    write_project_file(
        project_path,
        "rsconstruct.toml",
        ENCODING_WITH_PYTHON_ANALYZER,
    );
    write_project_file(project_path, "src/a.py", "import b\n");
    write_project_file(project_path, "src/b.py", "import c\n");
    write_project_file(project_path, "src/c.py", "import a\n");
    build_ok(project_path);

    let files = analyzer_files(project_path, "src/a.py");
    assert!(
        files.contains("src/b.py") && files.contains("src/c.py"),
        "{files}"
    );
    assert!(
        !files.contains("  src/a.py"),
        "a.py listed as its own dependency: {files}"
    );
}

/// A project with one `.c` source checked by `encoding` (a native checker
/// standing in for a compiler) and an icpp analyzer with `analyzer_extra`
/// appended to its stanza.
fn icpp_project(analyzer_extra: &str) -> TempDir {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    write_project_file(
        temp_dir.path(),
        "rsconstruct.toml",
        &format!(
            "[processor.checker.encoding]\nsrc_dirs = [\"src\"]\nsrc_extensions = [\".c\"]\n\n\
             [analyzer.icpp]\n{analyzer_extra}"
        ),
    );
    write_project_file(temp_dir.path(), "src/main.c", "int main;\n");
    temp_dir
}

/// Run `rsconstruct build`, which must fail; return its combined output.
fn build_fails(project_path: &std::path::Path) -> String {
    let out = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!out.status.success(), "build must fail: {combined}");
    combined
}

/// An unknown pkg-config package used to be printed and skipped: the
/// analyzer then resolved headers without that package's include
/// directories, and those headers silently fell out of every dependency
/// list. It must fail the build, naming the package.
#[test]
fn icpp_unknown_pkg_config_package_fails_the_build() {
    let temp_dir = icpp_project("pkg_config = [\"rsconstruct-no-such-package\"]\n");
    let output = build_fails(temp_dir.path());
    assert!(
        output.contains("rsconstruct-no-such-package"),
        "the error must name the package: {output}"
    );
}

/// A failing include-path command fails the build instead of being skipped.
#[test]
fn icpp_failing_include_path_command_fails_the_build() {
    let temp_dir = icpp_project("include_path_commands = [\"echo broken >&2; exit 3\"]\n");
    let output = build_fails(temp_dir.path());
    assert!(
        output.contains("echo broken >&2; exit 3") && output.contains("broken"),
        "the error must name the command and show its stderr: {output}"
    );
}

/// An include-path command must print an existing directory; anything else
/// used to be dropped (reported only under -v).
#[test]
fn icpp_include_path_command_must_print_a_directory() {
    let temp_dir = icpp_project("include_path_commands = [\"echo no/such/dir\"]\n");
    let output = build_fails(temp_dir.path());
    assert!(
        output.contains("not a directory") && output.contains("no/such/dir"),
        "the error must say what was printed: {output}"
    );
}

/// The success path: the directory a command prints is searched. icpp fails
/// on an unresolved quoted include, so a green build proves `v.h` was found
/// through the command's directory.
#[test]
fn icpp_include_path_command_directory_is_searched() {
    let temp_dir = icpp_project("include_path_commands = [\"echo vendor\"]\n");
    write_project_file(temp_dir.path(), "src/main.c", "#include \"v.h\"\n");
    write_project_file(temp_dir.path(), "vendor/v.h", "#define V 1\n");
    build_ok(temp_dir.path());
}
