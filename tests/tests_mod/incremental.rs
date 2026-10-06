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
    let path_env = format!(
        "{}:{}",
        project.join("toolbin").display(),
        std::env::var("PATH").unwrap_or_default()
    );
    run_rsconstruct_json_with_env(
        project,
        &["build"],
        &[("NO_COLOR", "1"), ("PATH", &path_env)],
    )
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
