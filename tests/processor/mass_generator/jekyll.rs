use std::fs;
use std::path::Path;

use crate::common::{make_executable, require_tool, run_rsconstruct_with_env, write_file};

test_checker!(jekyll, tool: "jekyll", processor: "processor.mass_generator.jekyll", no_project);

/// The real `jekyll` (planner and builder both) on a two-page site: the
/// build writes every planned page, and a second build has nothing to do.
#[test]
fn jekyll_builds_a_real_site() {
    require_tool("jekyll");
    require_tool("ruby");
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path();
    write_file(
        root,
        "rsconstruct.toml",
        "[processor.mass_generator.jekyll]\nsrc_dirs = [\".\"]\n",
    );
    write_file(root, "site/_config.yml", "title: Demo\n");
    write_file(root, "site/index.md", "---\ntitle: Home\n---\n# Home\n");
    write_file(root, "site/about.md", "---\ntitle: About\n---\n# About\n");

    let output = run_rsconstruct_with_env(root, &["build"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "build failed: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let index = fs::read_to_string(root.join("site/_site/index.html"))
        .expect("site/_site/index.html not produced");
    assert!(
        index.contains("Home"),
        "index.html lacks its heading: {index}"
    );
    assert!(root.join("site/_site/about.html").is_file());

    let output = run_rsconstruct_with_env(root, &["build", "-v"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("[processor.mass_generator.jekyll] Skipping (unchanged):"),
        "second build should skip: {stdout}"
    );
}

/// A stand-in for the `ruby` the planner runs with: it ignores the embedded
/// planner script and prints, as the plan, one `<page>.html` per `<page>.md`
/// in the site directory (argument 3), appending a line to `plans.log` per
/// run so a test can count how often the site was planned.
const FAKE_RUBY: &str = r#"#!/bin/bash
echo plan >> plans.log
src="$3"
printf '{"version":1,"files":['
first=1
for f in "$src"/*.md; do
  name=$(basename "$f" .md)
  [ "$first" = 1 ] || printf ','
  first=0
  printf '"%s.html"' "$name"
done
printf ']}\n'
"#;

/// A stand-in for `jekyll build -d <dest>`, run in the site directory:
/// writes one `<page>.html` per `<page>.md` into `<dest>`.
const FAKE_JEKYLL: &str = r#"#!/bin/bash
dest=""
while [ $# -gt 0 ]; do
  case "$1" in
    -d) dest="$2"; shift ;;
  esac
  shift
done
mkdir -p "$dest"
for f in *.md; do
  echo "<h1>$f</h1>" > "$dest/${f%.md}.html"
done
"#;

/// A site under `site/` whose planner and builder are the fakes above, plus
/// an explicit stanza that generates `site/extra.md` from `site/extra.src`.
fn setup_site_with_generated_page(root: &Path) {
    write_file(root, "fake-ruby.sh", FAKE_RUBY);
    make_executable(&root.join("fake-ruby.sh"));
    write_file(root, "fake-jekyll.sh", FAKE_JEKYLL);
    make_executable(&root.join("fake-jekyll.sh"));
    write_file(
        root,
        "gen.sh",
        "#!/bin/bash\ncp site/extra.src site/extra.md\n",
    );
    make_executable(&root.join("gen.sh"));
    write_file(root, "site/_config.yml", "title: Demo\n");
    write_file(root, "site/index.md", "# Home\n");
    write_file(root, "site/extra.src", "# Extra\n");
    let jekyll = root.join("fake-jekyll.sh");
    write_file(
        root,
        "rsconstruct.toml",
        &format!(
            "[processor.mass_generator.jekyll]\nsrc_dirs = [\".\"]\nruby = \"./fake-ruby.sh\"\ncommand = \"{}\"\nloose_manifest = true\n\n[processor.explicit.generic.extra]\ncommand = \"./gen.sh\"\ninputs = [\"site/extra.src\"]\noutput_files = [\"site/extra.md\"]\n",
            jekyll.display()
        ),
    );
}

fn plan_runs(root: &Path) -> usize {
    fs::read_to_string(root.join("plans.log")).map_or(0, |s| s.lines().count())
}

/// Discovery runs again when an upstream processor declares a file in the
/// site (`site/extra.md`, generated during the build). The site's plan is
/// keyed by the inputs it was computed from, so that round plans the site
/// again with the new file among its sources instead of replaying the plan
/// from the round that never saw it — which is what makes every page
/// depend on the generated file and the generator run first.
#[test]
fn jekyll_replans_when_an_upstream_generator_adds_a_site_file() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path();
    setup_site_with_generated_page(root);

    // The graph alone, before anything is built: round 0 plans the site
    // from disk, round 1 (with the virtual `site/extra.md`) plans it again.
    let output = run_rsconstruct_with_env(
        root,
        &["--json", "product", "show", "site/_site/index.html"],
        &[("NO_COLOR", "1")],
    );
    assert!(
        output.status.success(),
        "product show failed: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("invalid JSON: {e}\n---\n{stdout}"));
    let configured: Vec<&str> = parsed["inputs"]["configured"]
        .as_array()
        .unwrap_or_else(|| panic!("inputs.configured missing: {stdout}"))
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(
        configured.contains(&"site/extra.md"),
        "the generated page must be an input of every planned file: {configured:?}"
    );
    assert_eq!(
        plan_runs(root),
        2,
        "round 0 and the round that added site/extra.md each plan the site"
    );

    // The build then orders the generator before the site build.
    let output = run_rsconstruct_with_env(root, &["build"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "build failed: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(root.join("site/extra.md").is_file());
    assert!(root.join("site/_site/index.html").is_file());
}
