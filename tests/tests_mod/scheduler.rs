//! The executor's scheduler: products run as soon as their own
//! dependencies finish, under a hard `-j` cap and per-processor `max_jobs`.

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

/// `build` with `toolbin/` first on PATH and `extra` arguments, as JSON.
fn build(project: &Path, extra: &[&str]) -> crate::common::BuildResult {
    let path_env = format!(
        "{}:{}",
        project.join("toolbin").display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut args = vec!["build"];
    args.extend_from_slice(extra);
    run_rsconstruct_json_with_env(project, &args, &[("NO_COLOR", "1"), ("PATH", &path_env)])
}

/// A product runs once its own dependencies are done, not once every
/// product "before" it is. `slow` (no dependencies) waits for `flag/b.done`,
/// which `flag` writes after `first`, also dependency-free. Level by level,
/// `slow` and `first` share level 0 and `flag` is in level 1, so `flag`
/// cannot start until `slow` finishes — which waits for `flag`: `slow` times
/// out and fails. Scheduled as dependencies allow, with two workers, `flag`
/// runs as soon as `first` is done and `slow` completes.
#[test]
fn a_product_does_not_wait_for_unrelated_products() {
    let temp_dir = TempDir::new().unwrap();
    let project = temp_dir.path();

    write_tool(project, "copy", "cp \"$1\" \"$2\"\n");
    write_tool(
        project,
        "wait_for_flag",
        "i=0\n\
         while [ ! -f flag/b.done ]; do\n\
         \x20 i=$((i+1)); [ $i -gt 200 ] && { echo 'flag/b.done never appeared' >&2; exit 1; }\n\
         \x20 sleep 0.1\n\
         done\n\
         cp \"$1\" \"$2\"\n",
    );
    fs::write(
        project.join("rsconstruct.toml"),
        r#"[build]
hash_tool_versions = false

[processor.generator.generic.slow]
command = "wait_for_flag"
output_dir = "out/slow"
output_extension = "out"
batch = false
src_extensions = [".slow"]
src_dirs = ["src"]

[processor.generator.generic.first]
command = "copy"
output_dir = "mid"
output_extension = "b"
batch = false
src_extensions = [".first"]
src_dirs = ["src"]

[processor.generator.generic.flag]
command = "copy"
output_dir = "flag"
output_extension = "done"
batch = false
src_extensions = [".b"]
src_dirs = ["mid"]
"#,
    )
    .unwrap();
    fs::create_dir_all(project.join("src")).unwrap();
    fs::write(project.join("src/a.slow"), "a\n").unwrap();
    fs::write(project.join("src/b.first"), "b\n").unwrap();

    let result = build(project, &["-j", "2"]);
    assert!(
        result.exit_success,
        "the flag product must run while the slow one waits: {:?}",
        result.errors
    );
    assert_eq!(result.success, 3, "{result:?}");
}

/// `-j` caps every tool invocation, batches included, and `max_jobs` caps
/// one processor. Each invocation of `count` registers itself in
/// `running/<dir>/`, records how many invocations of any kind are running
/// at that moment, and how many of its own processor's, then sleeps so
/// invocations overlap. Two batching processors and one non-batching one
/// run under `-j 2`: at most two invocations may ever overlap. (Each batch
/// group used to get a thread of its own on top of `-j`.) The non-batching
/// one has `max_jobs = 1`: never two of its own at once.
#[test]
fn jobs_cap_every_invocation_and_max_jobs_caps_a_processor() {
    let temp_dir = TempDir::new().unwrap();
    let project = temp_dir.path();

    // $1 is the processor's tag; the rest are input/output pairs.
    write_tool(
        project,
        "count",
        "tag=$1; shift\n\
         mkdir -p running/all running/$tag\n\
         touch running/all/$$ running/$tag/$$\n\
         echo $(ls running/all | wc -l) >> concurrency.all\n\
         echo $(ls running/$tag | wc -l) >> concurrency.$tag\n\
         sleep 0.3\n\
         while [ $# -ge 2 ]; do cp \"$1\" \"$2\"; shift 2; done\n\
         rm running/all/$$ running/$tag/$$\n",
    );
    let stanza = |name: &str, ext: &str, batch: bool, max_jobs: Option<usize>| {
        format!(
            "[processor.generator.generic.{name}]\n\
             command = \"count\"\n\
             args = [\"{name}\"]\n\
             output_dir = \"out/{name}\"\n\
             output_extension = \"out\"\n\
             batch = {batch}\n\
             src_extensions = [\".{ext}\"]\n\
             src_dirs = [\"src\"]\n{}\n",
            max_jobs.map_or(String::new(), |n| format!("max_jobs = {n}\n"))
        )
    };
    fs::write(
        project.join("rsconstruct.toml"),
        format!(
            "[build]\nhash_tool_versions = false\nbatch_size = 2\n\n{}{}{}",
            stanza("left", "l", true, None),
            stanza("right", "r", true, None),
            stanza("single", "s", false, Some(1)),
        ),
    )
    .unwrap();
    fs::create_dir_all(project.join("src")).unwrap();
    for i in 0..4 {
        fs::write(project.join(format!("src/{i}.l")), "l\n").unwrap();
        fs::write(project.join(format!("src/{i}.r")), "r\n").unwrap();
        fs::write(project.join(format!("src/{i}.s")), "s\n").unwrap();
    }

    let result = build(project, &["-j", "2"]);
    assert!(result.exit_success, "{:?}", result.errors);
    assert_eq!(result.success, 12, "{result:?}");

    let max_of = |file: &str| -> usize {
        fs::read_to_string(project.join(file))
            .unwrap_or_else(|e| panic!("{file}: {e}"))
            .lines()
            .map(|l| l.trim().parse::<usize>().unwrap())
            .max()
            .unwrap()
    };
    assert!(
        max_of("concurrency.all") <= 2,
        "more invocations overlapped than -j 2 allows: {}",
        max_of("concurrency.all")
    );
    assert_eq!(max_of("concurrency.single"), 1, "max_jobs = 1 was exceeded");
}

/// A batching checker runs once over all its files even when they become
/// ready at different moments. `gen/b.chk` is generated from `src/b.gen`,
/// so its check waits for the generator while `src/a.chk` is ready from the
/// start. Dispatched as they become ready, the checker would run twice, the
/// two runs overlapping — and pytest over a wave with no test in it collects
/// nothing and exits 5, two mypy processes race on `.mypy_cache`. The
/// checker's products are held until the last of them is ready, then run as
/// one batch: `count_args` logs a single invocation with two files.
#[test]
fn a_batching_checker_waits_for_all_its_files() {
    let temp_dir = TempDir::new().unwrap();
    let project = temp_dir.path();

    write_tool(project, "copy", "cp \"$1\" \"$2\"\n");
    write_tool(project, "count_args", "echo \"$#\" >> invocations.log\n");
    fs::write(
        project.join("rsconstruct.toml"),
        r#"[build]
hash_tool_versions = false

[processor.generator.generic]
command = "copy"
output_dir = "gen"
output_extension = "chk"
batch = false
src_extensions = [".gen"]
src_dirs = ["src"]

[processor.checker.script]
command = "count_args"
src_extensions = [".chk"]
src_dirs = ["src", "gen"]
"#,
    )
    .unwrap();
    fs::create_dir_all(project.join("src")).unwrap();
    fs::create_dir_all(project.join("gen")).unwrap();
    fs::write(project.join("src/a.chk"), "a\n").unwrap();
    fs::write(project.join("src/b.gen"), "b\n").unwrap();
    // Left by an earlier build and stale: the checker discovers it, and its
    // product depends on the generator's.
    fs::write(project.join("gen/b.chk"), "old\n").unwrap();

    let result = build(project, &["-j", "4"]);
    assert!(result.exit_success, "{:?}", result.errors);
    assert_eq!(result.success, 3, "{result:?}");
    let log = fs::read_to_string(project.join("invocations.log")).unwrap();
    assert_eq!(log, "2\n", "one invocation over both files, got {log:?}");
}
