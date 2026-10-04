//! The mass generator: a tool that enumerates its outputs before producing
//! them. These tests drive a small bash "site generator" with a `plan` mode
//! (prints the manifest) and a `build` mode (writes `_site/<page>.html` for
//! every `docs/<page>.md`, appending a line to `runs.log` per invocation so
//! the tests can count how often the tool actually ran).

use crate::common::{
    make_executable, run_rsconstruct_json_with_env, run_rsconstruct_with_env, write_file,
};
use std::fs;
use std::path::Path;
use tempfile::TempDir;

const SITE_SCRIPT: &str = r#"#!/bin/bash
set -e
case "$1" in
  plan)
    # Counted so the plan-cache tests can tell a cached plan from a fresh one.
    echo plan >> plans.log
    echo '{"version":1,"outputs":['
    first=1
    for f in docs/*.md; do
      name=$(basename "$f" .md)
      [ "$first" = 1 ] || echo ','
      first=0
      printf '{"path":"_site/%s.html","sources":["%s","site.toml"]}' "$name" "$f"
    done
    echo ']}'
    ;;
  build)
    echo run >> runs.log
    mkdir -p _site
    for f in docs/*.md; do
      name=$(basename "$f" .md)
      { echo "<h1>$name</h1>"; cat "$f"; cat site.toml; } > "_site/$name.html"
    done
    # Misbehaviour switches, so tests can make the tool break its own plan.
    [ -f .write_extra ] && echo stray > _site/extra.html
    [ -f .skip_b ] && rm -f _site/b.html
    exit 0
    ;;
  *)
    echo "unknown mode $1" >&2
    exit 2
    ;;
esac
"#;

const CONFIG: &str = r#"[processor.mass_generator.generic.site]
command = "./site.sh"
args = ["build"]
predict_command = "./site.sh"
predict_args = ["plan"]
output_dirs = ["_site"]
"#;

fn setup_site_project(config: &str) -> TempDir {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let root = temp_dir.path();
    write_file(root, "site.sh", SITE_SCRIPT);
    make_executable(&root.join("site.sh"));
    write_file(root, "site.toml", "title = \"Demo\"\n");
    write_file(root, "docs/a.md", "page a\n");
    write_file(root, "docs/b.md", "page b\n");
    write_file(root, "rsconstruct.toml", config);
    temp_dir
}

fn tool_runs(root: &Path) -> usize {
    fs::read_to_string(root.join("runs.log")).map_or(0, |s| s.lines().count())
}

fn combined_output(output: &std::process::Output) -> String {
    format!(
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn build_ok(root: &Path) -> crate::common::BuildResult {
    let result = run_rsconstruct_json_with_env(root, &["build"], &[("NO_COLOR", "1")]);
    assert!(
        result.exit_success,
        "build should succeed: {:?}",
        result.errors
    );
    result
}

#[test]
fn mass_generator_builds_every_predicted_file_with_one_tool_run() {
    let temp_dir = setup_site_project(CONFIG);
    let root = temp_dir.path();

    let result = build_ok(root);

    assert_eq!(result.total_products, 2, "one product per predicted file");
    assert_eq!(result.count_status("success"), 2);
    // The build log names a product by its primary input (the page source).
    assert!(result.has_product("docs/a.md", "success"));
    assert!(result.has_product("docs/b.md", "success"));
    assert_eq!(
        fs::read_to_string(root.join("_site/a.html")).unwrap(),
        "<h1>a</h1>\npage a\ntitle = \"Demo\"\n"
    );
    assert!(root.join("_site/b.html").exists());
    assert_eq!(
        tool_runs(root),
        1,
        "two dirty products, one tool invocation"
    );
}

#[test]
fn mass_generator_skips_everything_when_unchanged() {
    let temp_dir = setup_site_project(CONFIG);
    let root = temp_dir.path();
    build_ok(root);

    let result = build_ok(root);

    assert_eq!(result.skipped, 2, "{:?}", result.products);
    assert_eq!(tool_runs(root), 1, "the tool must not run for a clean site");
}

#[test]
fn mass_generator_reruns_tool_once_when_one_source_changes() {
    let temp_dir = setup_site_project(CONFIG);
    let root = temp_dir.path();
    build_ok(root);

    write_file(root, "docs/a.md", "page a, revised\n");
    let result = build_ok(root);

    assert!(
        result.has_product("docs/a.md", "success"),
        "{:?}",
        result.products
    );
    assert!(
        result.has_product("docs/b.md", "skipped"),
        "b's inputs did not change: {:?}",
        result.products
    );
    assert_eq!(tool_runs(root), 2, "exactly one more invocation");
    assert_eq!(
        fs::read_to_string(root.join("_site/a.html")).unwrap(),
        "<h1>a</h1>\npage a, revised\ntitle = \"Demo\"\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("_site/b.html")).unwrap(),
        "<h1>b</h1>\npage b\ntitle = \"Demo\"\n",
        "the tool rewrote b.html too; it must still be intact"
    );
}

#[test]
fn mass_generator_shared_source_dirties_every_dependent_page() {
    let temp_dir = setup_site_project(CONFIG);
    let root = temp_dir.path();
    build_ok(root);

    // site.toml is a source of every page.
    write_file(root, "site.toml", "title = \"Renamed\"\n");
    let result = build_ok(root);

    assert_eq!(result.count_status("success"), 2, "{:?}", result.products);
    assert_eq!(tool_runs(root), 2);
}

#[test]
fn mass_generator_restores_from_cache_without_running_tool() {
    let temp_dir = setup_site_project(CONFIG);
    let root = temp_dir.path();
    build_ok(root);

    let clean = run_rsconstruct_with_env(root, &["clean", "outputs"], &[("NO_COLOR", "1")]);
    assert!(clean.status.success(), "{}", combined_output(&clean));
    assert!(!root.join("_site/a.html").exists());
    assert!(!root.join("_site/b.html").exists());

    let result = build_ok(root);

    assert_eq!(result.restored, 2, "{:?}", result.products);
    assert_eq!(tool_runs(root), 1, "a restore must not invoke the tool");
    assert_eq!(
        fs::read_to_string(root.join("_site/b.html")).unwrap(),
        "<h1>b</h1>\npage b\ntitle = \"Demo\"\n"
    );
}

#[test]
fn mass_generator_clean_removes_predicted_files_only() {
    let temp_dir = setup_site_project(CONFIG);
    let root = temp_dir.path();
    build_ok(root);
    // A file nobody predicted, living in the shared directory.
    write_file(root, "_site/CNAME", "example.org\n");

    let clean = run_rsconstruct_with_env(root, &["clean", "outputs"], &[("NO_COLOR", "1")]);
    assert!(clean.status.success(), "{}", combined_output(&clean));

    assert!(!root.join("_site/a.html").exists());
    assert!(!root.join("_site/b.html").exists());
    assert!(
        root.join("_site/CNAME").exists(),
        "clean removes declared files, never the directory"
    );
}

#[test]
fn mass_generator_new_source_becomes_a_new_product() {
    let temp_dir = setup_site_project(CONFIG);
    let root = temp_dir.path();
    build_ok(root);

    write_file(root, "docs/c.md", "page c\n");
    let result = build_ok(root);

    assert_eq!(result.total_products, 3, "{:?}", result.products);
    assert!(result.has_product("docs/c.md", "success"));
    assert!(root.join("_site/c.html").exists());
    assert_eq!(tool_runs(root), 2);
}

#[test]
fn mass_generator_strict_mode_rejects_off_plan_file() {
    let temp_dir = setup_site_project(CONFIG);
    let root = temp_dir.path();
    write_file(root, ".write_extra", "");

    let output = run_rsconstruct_with_env(root, &["build"], &[("NO_COLOR", "1")]);

    assert!(!output.status.success(), "{}", combined_output(&output));
    let text = combined_output(&output);
    assert!(
        text.contains("unexpected: _site/extra.html (written but not in the plan)"),
        "{text}"
    );
    assert!(text.contains("do not match the plan"), "{text}");
}

#[test]
fn mass_generator_strict_mode_rejects_missing_predicted_file() {
    let temp_dir = setup_site_project(CONFIG);
    let root = temp_dir.path();
    write_file(root, ".skip_b", "");

    let output = run_rsconstruct_with_env(root, &["build"], &[("NO_COLOR", "1")]);

    assert!(!output.status.success(), "{}", combined_output(&output));
    let text = combined_output(&output);
    assert!(
        text.contains("missing: _site/b.html (predicted but not produced)"),
        "{text}"
    );
}

#[test]
fn mass_generator_tool_failure_is_reported_once_not_rerun() {
    let temp_dir = setup_site_project(CONFIG);
    let root = temp_dir.path();
    write_file(root, ".skip_b", "");

    let result =
        run_rsconstruct_json_with_env(root, &["build", "--keep-going"], &[("NO_COLOR", "1")]);

    assert!(!result.exit_success);
    assert_eq!(
        tool_runs(root),
        1,
        "the second product must not start the tool again"
    );
    let errors = result.errors.join("\n");
    assert!(
        errors.contains("already failed earlier in this build"),
        "{errors}"
    );
}

#[test]
fn mass_generator_loose_manifest_downgrades_mismatch_to_warning() {
    let config = format!("{CONFIG}loose_manifest = true\n");
    let temp_dir = setup_site_project(&config);
    let root = temp_dir.path();
    write_file(root, ".write_extra", "");

    let output = run_rsconstruct_with_env(root, &["build"], &[("NO_COLOR", "1")]);

    assert!(output.status.success(), "{}", combined_output(&output));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Warning:"), "{stderr}");
    assert!(
        stderr.contains("unexpected: _site/extra.html (written but not in the plan)"),
        "{stderr}"
    );
    assert!(stderr.contains("loose_manifest = true"), "{stderr}");
    assert!(root.join("_site/a.html").exists());
}

#[test]
fn mass_generator_rejects_manifest_path_outside_output_dirs() {
    let config = CONFIG.replace("output_dirs = [\"_site\"]", "output_dirs = [\"public\"]");
    let temp_dir = setup_site_project(&config);
    let root = temp_dir.path();

    let output = run_rsconstruct_with_env(root, &["build"], &[("NO_COLOR", "1")]);

    assert!(!output.status.success(), "{}", combined_output(&output));
    let text = combined_output(&output);
    assert!(
        text.contains(
            "manifest output path '_site/a.html' is outside every output_dirs entry (public)"
        ),
        "{text}"
    );
    assert_eq!(
        tool_runs(root),
        0,
        "a rejected plan must never reach the build command"
    );
}

#[test]
fn mass_generator_reports_failed_plan_command() {
    let temp_dir = setup_site_project(CONFIG);
    let root = temp_dir.path();
    write_file(
        root,
        "plan.sh",
        "#!/bin/bash\necho 'plan exploded' >&2\nexit 3\n",
    );
    make_executable(&root.join("plan.sh"));
    let config = CONFIG.replace(
        "predict_command = \"./site.sh\"",
        "predict_command = \"./plan.sh\"",
    );
    write_file(root, "rsconstruct.toml", &config);

    let output = run_rsconstruct_with_env(root, &["build"], &[("NO_COLOR", "1")]);

    assert!(!output.status.success(), "{}", combined_output(&output));
    let text = combined_output(&output);
    assert!(
        text.contains("predict_command ./plan.sh plan failed"),
        "{text}"
    );
    assert!(text.contains("plan exploded"), "{text}");
}

fn plan_runs(root: &Path) -> usize {
    fs::read_to_string(root.join("plans.log")).map_or(0, |s| s.lines().count())
}

/// The scan fields declare what the plan is computed from (`docs/` and
/// `site.toml` here). While none of those files changes, later builds reuse
/// the cached plan instead of running `predict_command`.
const CACHED_CONFIG_EXTRA: &str =
    "src_dirs = [\"docs\"]\nsrc_extensions = [\".md\"]\nsrc_files = [\"site.toml\"]\n";

#[test]
fn mass_generator_reuses_cached_plan_while_inputs_are_unchanged() {
    let temp_dir = setup_site_project(&format!("{CONFIG}{CACHED_CONFIG_EXTRA}"));
    let root = temp_dir.path();

    build_ok(root);
    assert_eq!(plan_runs(root), 1);
    let result = build_ok(root);
    assert_eq!(result.skipped, 2, "{:?}", result.products);
    assert_eq!(
        plan_runs(root),
        1,
        "unchanged plan inputs: the plan must come from cache"
    );
    assert!(
        root.join(".rsconstruct/mass_generator/processor.mass_generator.generic.site.json")
            .is_file()
    );
}

#[test]
fn mass_generator_replans_when_a_plan_input_changes() {
    let temp_dir = setup_site_project(&format!("{CONFIG}{CACHED_CONFIG_EXTRA}"));
    let root = temp_dir.path();
    build_ok(root);

    // A new page is a new file under a declared plan input.
    write_file(root, "docs/c.md", "page c\n");
    let result = build_ok(root);
    assert_eq!(
        plan_runs(root),
        2,
        "a new source file must trigger a fresh plan"
    );
    assert_eq!(result.total_products, 3, "{:?}", result.products);
    assert!(result.has_product("docs/c.md", "success"));

    // An edit to a declared plan input re-plans too.
    write_file(root, "site.toml", "title = \"Renamed\"\n");
    build_ok(root);
    assert_eq!(plan_runs(root), 3);
}

#[test]
fn mass_generator_without_plan_inputs_plans_every_time() {
    let temp_dir = setup_site_project(CONFIG);
    let root = temp_dir.path();
    build_ok(root);
    build_ok(root);
    assert_eq!(
        plan_runs(root),
        2,
        "nothing declares the plan inputs, so nothing may be cached"
    );
    assert!(!root.join(".rsconstruct/mass_generator").exists());
}

#[test]
fn mass_generator_rejects_plan_input_dirs_without_extensions() {
    let temp_dir = setup_site_project(&format!("{CONFIG}src_dirs = [\"docs\"]\n"));
    let output = run_rsconstruct_with_env(temp_dir.path(), &["build"], &[("NO_COLOR", "1")]);
    assert!(!output.status.success(), "{}", combined_output(&output));
    assert!(
        combined_output(&output).contains("src_extensions is empty"),
        "{}",
        combined_output(&output)
    );
}

#[test]
fn mass_generator_rejects_corrupt_plan_cache() {
    let temp_dir = setup_site_project(&format!("{CONFIG}{CACHED_CONFIG_EXTRA}"));
    let root = temp_dir.path();
    build_ok(root);
    write_file(
        root,
        ".rsconstruct/mass_generator/processor.mass_generator.generic.site.json",
        "garbage",
    );

    let output = run_rsconstruct_with_env(root, &["build"], &[("NO_COLOR", "1")]);

    assert!(!output.status.success(), "{}", combined_output(&output));
    assert!(
        combined_output(&output).contains("is corrupt; delete it and rerun"),
        "{}",
        combined_output(&output)
    );
}

/// Another processor writes into `_site/` at the same time. Its file is a
/// declared output, so the plan check must not report it as off-plan.
#[test]
fn mass_generator_shares_output_dir_with_a_declared_neighbor() {
    let temp_dir = setup_site_project(CONFIG);
    let root = temp_dir.path();
    write_file(
        root,
        "about.sh",
        "#!/bin/bash\nset -e\nmkdir -p _site\necho 'about page' > _site/about.html\n",
    );
    make_executable(&root.join("about.sh"));
    write_file(root, "about.txt", "about\n");
    let config = format!(
        "{CONFIG}\n[processor.explicit.generic.about]\ncommand = \"./about.sh\"\ninputs = [\"about.txt\"]\noutput_files = [\"_site/about.html\"]\n"
    );
    write_file(root, "rsconstruct.toml", &config);

    let result = build_ok(root);

    assert_eq!(result.total_products, 3, "{:?}", result.products);
    assert_eq!(result.count_status("success"), 3, "{:?}", result.products);
    assert_eq!(
        fs::read_to_string(root.join("_site/about.html")).unwrap(),
        "about page\n"
    );
    assert!(root.join("_site/a.html").exists());
}

/// A predicted file is a real product output, so a downstream processor
/// that lists it as an input runs after the tool — the copy would fail if
/// it ran first.
#[test]
fn mass_generator_outputs_feed_downstream_processors() {
    let temp_dir = setup_site_project(CONFIG);
    let root = temp_dir.path();
    write_file(
        root,
        "report.sh",
        "#!/bin/bash\nset -e\ncp _site/a.html report.txt\n",
    );
    make_executable(&root.join("report.sh"));
    let config = format!(
        "{CONFIG}\n[processor.explicit.generic.report]\ncommand = \"./report.sh\"\ninputs = [\"_site/a.html\"]\noutput_files = [\"report.txt\"]\n"
    );
    write_file(root, "rsconstruct.toml", &config);

    let result = build_ok(root);

    assert_eq!(result.count_status("success"), 3, "{:?}", result.products);
    assert_eq!(
        fs::read_to_string(root.join("report.txt")).unwrap(),
        "<h1>a</h1>\npage a\ntitle = \"Demo\"\n"
    );

    // Change a's source: the report depends on a.html and must rebuild too.
    write_file(root, "docs/a.md", "page a, revised\n");
    let result = build_ok(root);
    assert!(
        result
            .products
            .iter()
            .any(|p| p.processor == "processor.explicit.generic.report" && p.status == "success"),
        "{:?}",
        result.products
    );
    assert_eq!(
        fs::read_to_string(root.join("report.txt")).unwrap(),
        "<h1>a</h1>\npage a, revised\ntitle = \"Demo\"\n"
    );
}

/// Several predicted files can depend on exactly the same sources (a tag's
/// index page, its `page/1/` redirect and its feed, say). The cache key must
/// still tell them apart: the first run of this scenario "restored" the
/// second file from the first file's blob and wrote the wrong content.
#[test]
fn mass_generator_identical_sources_keep_distinct_outputs() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let root = temp_dir.path();
    write_file(
        root,
        "twin.sh",
        concat!(
            "#!/bin/bash\nset -e\ncase \"$1\" in\n",
            "  plan) echo '{\"version\":1,\"outputs\":[",
            "{\"path\":\"out/index.html\",\"sources\":[\"src.md\"]},",
            "{\"path\":\"out/feed.xml\",\"sources\":[\"src.md\"]}]}' ;;\n",
            "  build) mkdir -p out; echo '<html>' > out/index.html; echo '<feed/>' > out/feed.xml ;;\n",
            "esac\n",
        ),
    );
    make_executable(&root.join("twin.sh"));
    write_file(root, "src.md", "source\n");
    write_file(
        root,
        "rsconstruct.toml",
        "[processor.mass_generator.generic.twin]\ncommand = \"./twin.sh\"\nargs = [\"build\"]\n\
         predict_command = \"./twin.sh\"\npredict_args = [\"plan\"]\noutput_dirs = [\"out\"]\n",
    );

    let result = build_ok(root);
    assert_eq!(result.count_status("success"), 2, "{:?}", result.products);
    assert_eq!(
        fs::read_to_string(root.join("out/feed.xml")).unwrap(),
        "<feed/>\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("out/index.html")).unwrap(),
        "<html>\n"
    );

    // And each restores its own bytes.
    let clean = run_rsconstruct_with_env(root, &["clean", "outputs"], &[("NO_COLOR", "1")]);
    assert!(clean.status.success(), "{}", combined_output(&clean));
    let result = build_ok(root);
    assert_eq!(result.restored, 2, "{:?}", result.products);
    assert_eq!(
        fs::read_to_string(root.join("out/feed.xml")).unwrap(),
        "<feed/>\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("out/index.html")).unwrap(),
        "<html>\n"
    );
}
