//! Incremental-build correctness: what a build caches, under which key, and
//! when a product's inputs are known.

use crate::common::{make_executable, run_rsconstruct_json_with_env};
use std::fs;
use std::path::Path;
use tempfile::TempDir;

/// Write `body` as an executable `/bin/sh` script `toolbin/<name>`.
fn write_tool(project: &Path, name: &str, body: &str) {
    let bin_dir = project.join("toolbin");
    fs::create_dir_all(&bin_dir).unwrap();
    let tool = bin_dir.join(name);
    fs::write(&tool, format!("#!/bin/sh\n{body}")).unwrap();
    make_executable(&tool);
}

/// Build with `toolbin/` first on PATH, as JSON.
fn build(project: &Path) -> crate::common::BuildResult {
    build_with(project, &[])
}

/// `build` plus `extra` arguments, with `toolbin/` first on PATH, as JSON.
fn build_with(project: &Path, extra: &[&str]) -> crate::common::BuildResult {
    let path_env = format!(
        "{}:{}",
        project.join("toolbin").display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut args = vec!["build"];
    args.extend_from_slice(extra);
    run_rsconstruct_json_with_env(project, &args, &[("NO_COLOR", "1"), ("PATH", &path_env)])
}

/// A two-step chain: `first` keeps only the first line of `src/a.src` (and
/// fails on an input containing `FAIL`), `second` copies its output.
fn first_line_chain() -> TempDir {
    let temp_dir = TempDir::new().unwrap();
    let project = temp_dir.path();
    write_tool(
        project,
        "first_line",
        "grep -q FAIL \"$1\" && { echo 'FAIL in input' >&2; exit 1; }\nhead -n 1 \"$1\" > \"$2\"\n",
    );
    write_tool(project, "copy", "cp \"$1\" \"$2\"\n");
    fs::write(
        project.join("rsconstruct.toml"),
        r#"[build]
hash_tool_versions = false

[processor.generator.generic.first]
command = "first_line"
output_dir = "mid"
output_extension = "mid"
batch = false
src_extensions = [".src"]
src_dirs = ["src"]

[processor.generator.generic.second]
command = "copy"
output_dir = "out/second"
output_extension = "out"
batch = false
src_extensions = [".mid"]
src_dirs = ["mid"]
"#,
    )
    .unwrap();
    fs::create_dir_all(project.join("src")).unwrap();
    fs::write(project.join("src/a.src"), "title\n").unwrap();
    temp_dir
}

fn status_of<'a>(result: &'a crate::common::BuildResult, processor: &str) -> Option<&'a str> {
    result
        .products
        .iter()
        .find(|p| p.processor == processor)
        .map(|p| p.status.as_str())
}

/// Unchanged-output pruning: when a product rebuilds to identical bytes,
/// the products reading it are skipped, outputs untouched — not re-run, and
/// not restored from the cache either. (Their outputs used to be deleted
/// before the run because a dependency was predicted to change, so they
/// ended as a restore.)
#[test]
fn rebuilding_to_identical_bytes_skips_the_dependents() {
    let temp_dir = first_line_chain();
    let project = temp_dir.path();

    let first = build(project);
    assert!(first.exit_success, "{:?}", first.errors);
    assert_eq!(first.success, 2);

    // The first line is unchanged, so `first` writes the same bytes.
    fs::write(project.join("src/a.src"), "title\nmore\n").unwrap();
    let second = build(project);
    assert!(second.exit_success, "{:?}", second.errors);
    assert_eq!(
        status_of(&second, "processor.generator.generic.first"),
        Some("success")
    );
    assert_eq!(
        status_of(&second, "processor.generator.generic.second"),
        Some("skipped"),
        "an unchanged dependency must not rerun or restore its consumer: {second:?}"
    );
    assert_eq!(second.restored, 0, "{second:?}");
}

/// When a product fails, what depends on it never runs — and must not keep
/// its stale output either, or the tree would look built. Outputs are no
/// longer removed before the run, so this is checked at the end of it, for
/// both a build that stops at the failure and one that keeps going.
#[test]
fn a_failed_dependency_leaves_no_stale_downstream_output() {
    for keep_going in [false, true] {
        let temp_dir = first_line_chain();
        let project = temp_dir.path();
        let downstream = project.join("out/second/a.out");

        let first = build(project);
        assert!(first.exit_success, "{:?}", first.errors);
        assert!(downstream.exists());

        fs::write(project.join("src/a.src"), "FAIL\n").unwrap();
        let args: &[&str] = if keep_going { &["--keep-going"] } else { &[] };
        let second = build_with(project, args);
        assert!(!second.exit_success, "the first step must fail");
        assert!(
            !downstream.exists(),
            "keep_going = {keep_going}: the stale downstream output must be removed"
        );
    }
}

/// A source that one product generates and an analyzer scans is analyzed
/// after its producer runs, not only at graph time.
///
/// `gen/doc.md` is generated, and the markdown analyzer finds that it
/// references `img/pic.png` — itself generated, two steps down another
/// chain. On a clean checkout `doc.md` does not exist when the graph is
/// built, so its reference is only discovered mid-build. The page product
/// must then (1) wait for `img/pic.png`, which it reads (the `needpic` tool
/// fails without it), and (2) be cached under a key that includes the
/// picture, so the very next build finds everything up to date.
#[test]
fn generated_source_is_analyzed_before_its_consumer_runs() {
    let temp_dir = TempDir::new().unwrap();
    let project = temp_dir.path();

    write_tool(
        project,
        "mkdoc",
        "{ cat \"$1\"; echo; echo '![p](img/pic.png)'; } > \"$2\"\n",
    );
    write_tool(project, "copy", "cp \"$1\" \"$2\"\n");
    write_tool(
        project,
        "needpic",
        "test -f img/pic.png || { echo 'img/pic.png missing' >&2; exit 1; }\ncp \"$1\" \"$2\"\n",
    );

    fs::write(
        project.join("rsconstruct.toml"),
        r#"[build]
hash_tool_versions = false

[analyzer.markdown]

[processor.generator.generic.doc]
command = "mkdoc"
output_dir = "gen"
output_extension = "md"
batch = false
src_extensions = [".txt"]
src_dirs = ["src"]

[processor.generator.generic.stage]
command = "copy"
output_dir = "stage"
output_extension = "mid"
batch = false
src_extensions = [".raw"]
src_dirs = ["raw"]

[processor.generator.generic.pic]
command = "copy"
output_dir = "img"
output_extension = "png"
batch = false
src_extensions = [".mid"]
src_dirs = ["stage"]

[processor.generator.generic.page]
command = "needpic"
output_dir = "out/page"
output_extension = "html"
batch = false
src_extensions = [".md"]
src_dirs = ["gen"]
"#,
    )
    .unwrap();
    fs::create_dir_all(project.join("src")).unwrap();
    fs::create_dir_all(project.join("raw")).unwrap();
    fs::write(project.join("src/doc.txt"), "# Doc\n").unwrap();
    fs::write(project.join("raw/pic.raw"), "pixels v1\n").unwrap();
    // The markdown analyzer only activates when the tree has a .md file.
    fs::write(project.join("README.md"), "# readme\n").unwrap();

    let first = build(project);
    assert!(
        first.exit_success,
        "clean build must order the page after the picture it references: {:?}",
        first.errors
    );
    assert_eq!(first.success, 4, "all four products build: {first:?}");

    let second = build(project);
    assert!(second.exit_success, "{:?}", second.errors);
    assert_eq!(
        second.success, 0,
        "the page was cached under its full input set, so nothing reruns: {second:?}"
    );

    // The picture is a real input of the page now.
    fs::write(project.join("raw/pic.raw"), "pixels v2\n").unwrap();
    let third = build(project);
    assert!(third.exit_success, "{:?}", third.errors);
    let status_of = |processor: &str| {
        third
            .products
            .iter()
            .find(|p| p.processor == processor)
            .map(|p| p.status.as_str())
    };
    assert_eq!(
        status_of("processor.generator.generic.page"),
        Some("success"),
        "a changed picture must rebuild the page: {third:?}"
    );
    assert_eq!(
        status_of("processor.generator.generic.doc"),
        Some("skipped"),
        "the document itself did not change: {third:?}"
    );
}

/// Build a project whose `use` product loads `need`, a file the `make`
/// product generates and which does not exist before the first build, then
/// check the analyzer ordered `use` after `make`, cached it under its full
/// input set, and rebuilds it when the generated file changes.
fn assert_generated_reference_resolves(project: &Path, maker_input: &str, need: &str) {
    let first = build(project);
    assert!(
        first.exit_success,
        "clean build must order the consumer after the file it loads: {:?}",
        first.errors
    );
    assert_eq!(first.success, 2, "both products build: {first:?}");

    let second = build(project);
    assert!(second.exit_success, "{:?}", second.errors);
    assert_eq!(
        second.success, 0,
        "the consumer was cached under its full input set: {second:?}"
    );

    fs::write(project.join(maker_input), "v2\n").unwrap();
    let third = build(project);
    assert!(third.exit_success, "{:?}", third.errors);
    let status_of = |processor: &str| {
        third
            .products
            .iter()
            .find(|p| p.processor == processor)
            .map(|p| p.status.as_str())
    };
    assert_eq!(
        status_of("processor.generator.generic.use"),
        Some("success"),
        "a changed {need} must rebuild its consumer: {third:?}"
    );
}

/// Build a generator whose output names its input, rename the input
/// without changing its content, and build again. Returns the second
/// build and the renamed output's text.
fn rename_with_name_in_output(name_in_key: bool) -> (crate::common::BuildResult, String) {
    let temp_dir = TempDir::new().unwrap();
    let project = temp_dir.path();
    write_tool(
        project,
        "stamp",
        "{ echo \"generated from $1\"; cat \"$1\"; } > \"$2\"\n",
    );
    fs::write(
        project.join("rsconstruct.toml"),
        format!(
            r#"[build]
hash_tool_versions = false

[processor.generator.generic.stamp]
command = "stamp"
output_dir = "out/stamp"
output_extension = "txt"
batch = false
src_extensions = [".src"]
src_dirs = ["src"]
output_depends_on_input_name = {name_in_key}
"#
        ),
    )
    .unwrap();
    fs::create_dir_all(project.join("src")).unwrap();
    fs::write(project.join("src/a.src"), "same content\n").unwrap();

    let first = build(project);
    assert!(first.exit_success, "{:?}", first.errors);
    assert_eq!(first.success, 1, "{first:?}");
    let unchanged = build(project);
    assert_eq!(
        unchanged.success + unchanged.restored,
        0,
        "an unchanged input is skipped either way: {unchanged:?}"
    );

    fs::rename(project.join("src/a.src"), project.join("src/b.src")).unwrap();
    let renamed = build(project);
    assert!(renamed.exit_success, "{:?}", renamed.errors);
    let text = fs::read_to_string(project.join("out/stamp/b.txt")).unwrap();
    (renamed, text)
}

/// By default the cache key is path-free: a renamed input with the same
/// content restores the cached output, even one that names the old file.
#[test]
fn renamed_input_restores_by_default() {
    let (renamed, text) = rename_with_name_in_output(false);
    assert_eq!(renamed.restored, 1, "{renamed:?}");
    assert!(
        text.contains("generated from src/a.src"),
        "the restored output still names the old file: {text}"
    );
}

/// `output_depends_on_input_name` puts the input paths in the cache key, so
/// the renamed input is rebuilt and its output names the new file.
#[test]
fn output_depends_on_input_name_rebuilds_renamed_input() {
    let (renamed, text) = rename_with_name_in_output(true);
    assert_eq!(renamed.success, 1, "rebuilt, not restored: {renamed:?}");
    assert_eq!(renamed.restored, 0, "{renamed:?}");
    assert!(
        text.contains("generated from src/b.src"),
        "the output names the renamed file: {text}"
    );
}

/// The sass analyzer resolves a `@use` of a partial another product
/// generates, through the declared outputs in the file index.
#[test]
fn sass_use_of_generated_partial_is_a_dependency() {
    let temp_dir = TempDir::new().unwrap();
    let project = temp_dir.path();

    write_tool(project, "copy", "cp \"$1\" \"$2\"\n");
    write_tool(
        project,
        "needpartial",
        "test -f partials/_colors.scss || { echo 'partials/_colors.scss missing' >&2; exit 1; }\ncp \"$1\" \"$2\"\n",
    );
    fs::write(
        project.join("rsconstruct.toml"),
        r#"[build]
hash_tool_versions = false

[analyzer.sass]
load_paths = ["partials"]

[processor.generator.generic.make]
command = "copy"
output_dir = "partials"
output_extension = "scss"
batch = false
src_extensions = [".pal"]
src_dirs = ["pal"]

[processor.generator.generic.use]
command = "needpartial"
output_dir = "out/css"
output_extension = "css"
batch = false
src_extensions = [".scss"]
src_dirs = ["styles"]
"#,
    )
    .unwrap();
    fs::create_dir_all(project.join("pal")).unwrap();
    fs::create_dir_all(project.join("styles")).unwrap();
    fs::write(project.join("pal/_colors.pal"), "v1\n").unwrap();
    fs::write(project.join("styles/main.scss"), "@use \"colors\";\n").unwrap();

    assert_generated_reference_resolves(project, "pal/_colors.pal", "partial");
}

/// The tera analyzer resolves an `{% include %}` of a template another
/// product generates, through the declared outputs in the file index.
#[test]
fn tera_include_of_generated_template_is_a_dependency() {
    let temp_dir = TempDir::new().unwrap();
    let project = temp_dir.path();

    write_tool(project, "copy", "cp \"$1\" \"$2\"\n");
    write_tool(
        project,
        "needpart",
        "test -f gen/part.tera || { echo 'gen/part.tera missing' >&2; exit 1; }\ncp \"$1\" \"$2\"\n",
    );
    fs::write(
        project.join("rsconstruct.toml"),
        r#"[build]
hash_tool_versions = false

[analyzer.tera]

[processor.generator.generic.make]
command = "copy"
output_dir = "gen"
output_extension = "tera"
batch = false
src_extensions = [".part"]
src_dirs = ["parts"]

[processor.generator.generic.use]
command = "needpart"
output_dir = "out/pages"
output_extension = "html"
batch = false
src_extensions = [".tera"]
src_dirs = ["pages"]
"#,
    )
    .unwrap();
    fs::create_dir_all(project.join("parts")).unwrap();
    fs::create_dir_all(project.join("pages")).unwrap();
    fs::write(project.join("parts/part.part"), "v1\n").unwrap();
    fs::write(
        project.join("pages/index.tera"),
        "{% include \"gen/part.tera\" %}\n",
    )
    .unwrap();

    assert_generated_reference_resolves(project, "parts/part.part", "template");
}

/// A product whose `dep_inputs` glob matches its own output does not depend
/// on that output. Otherwise the output joins the inputs once it exists, and
/// the second build sees a different key than the first and reruns.
#[test]
fn own_output_matched_by_dep_inputs_is_not_an_input() {
    let temp_dir = TempDir::new().unwrap();
    let project = temp_dir.path();

    write_tool(project, "copy", "cp \"$1\" \"$2\"\n");
    fs::write(
        project.join("rsconstruct.toml"),
        r#"[build]
hash_tool_versions = false

[processor.generator.generic]
command = "copy"
output_dir = "out/g"
output_extension = "txt"
batch = false
src_extensions = [".src"]
src_dirs = ["src"]
dep_inputs = ["out/g/*.txt"]
"#,
    )
    .unwrap();
    fs::create_dir_all(project.join("src")).unwrap();
    fs::write(project.join("src/a.src"), "a\n").unwrap();

    let first = build(project);
    assert!(first.exit_success, "{:?}", first.errors);
    assert_eq!(first.success, 1);

    let second = build(project);
    assert!(second.exit_success, "{:?}", second.errors);
    assert_eq!(
        second.success, 0,
        "the product's own output must not make its key unstable: {second:?}"
    );
    assert_eq!(second.skipped, 1, "{second:?}");
}

/// An input rewritten with its mtime restored (`cp -p`, `tar x`,
/// `rsync -a`) must still rebuild. The mtime cache used to trust a
/// matching mtime and serve the old checksum, so the build skipped with an
/// output made from the old content.
#[test]
fn input_rewritten_with_mtime_restored_rebuilds() {
    let temp_dir = TempDir::new().unwrap();
    let project = temp_dir.path();

    write_tool(project, "copy", "cp \"$1\" \"$2\"\n");
    fs::write(
        project.join("rsconstruct.toml"),
        r#"[build]
hash_tool_versions = false

[processor.generator.generic]
command = "copy"
output_dir = "out/g"
output_extension = "txt"
batch = false
src_extensions = [".src"]
src_dirs = ["src"]
"#,
    )
    .unwrap();
    fs::create_dir_all(project.join("src")).unwrap();
    let input = project.join("src/a.src");
    fs::write(&input, "aaaa\n").unwrap();
    let old_mtime = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
    let set_old_mtime = || {
        fs::File::options()
            .write(true)
            .open(&input)
            .unwrap()
            .set_modified(old_mtime)
            .unwrap();
    };
    set_old_mtime();
    // Files changed within the last two seconds (by mtime or ctime) are
    // never stored in the mtime cache; wait so the first build stores one.
    std::thread::sleep(std::time::Duration::from_millis(2_500));

    let first = build(project);
    assert!(first.exit_success, "{:?}", first.errors);
    assert_eq!(first.success, 1);

    // Same length, different bytes, mtime put back.
    fs::write(&input, "bbbb\n").unwrap();
    set_old_mtime();

    let second = build(project);
    assert!(second.exit_success, "{:?}", second.errors);
    assert_eq!(
        second.success, 1,
        "rewritten content must rebuild despite the restored mtime: {second:?}"
    );
    assert_eq!(
        fs::read_to_string(project.join("out/g/a.txt")).unwrap(),
        "bbbb\n"
    );
}

/// Outputs are cached under the inputs the tool started on. When an input
/// changes while the tool runs, the outputs match neither version: they are
/// not cached, and the next build runs the product again. (Caching them
/// under the edited content, as keying by a post-run checksum did, makes
/// the next build skip with outputs built from the old content.)
#[test]
fn input_edited_while_running_is_not_cached() {
    let temp_dir = TempDir::new().unwrap();
    let project = temp_dir.path();

    // Copies its input, then edits it — standing in for a user saving the
    // file while the build runs.
    write_tool(
        project,
        "copy_then_touch",
        "cp \"$1\" \"$2\"\necho edited >> \"$1\"\n",
    );
    fs::write(
        project.join("rsconstruct.toml"),
        r#"[build]
hash_tool_versions = false

[processor.generator.generic]
command = "copy_then_touch"
output_dir = "out/g"
output_extension = "txt"
batch = false
src_extensions = [".src"]
src_dirs = ["src"]
"#,
    )
    .unwrap();
    fs::create_dir_all(project.join("src")).unwrap();
    fs::write(project.join("src/a.src"), "a\n").unwrap();

    let first = build(project);
    assert!(first.exit_success, "{:?}", first.errors);
    assert_eq!(first.success, 1);

    let second = build(project);
    assert!(second.exit_success, "{:?}", second.errors);
    assert_eq!(
        second.success, 1,
        "outputs built from content that was edited mid-run must not be reused: {second:?}"
    );
}
