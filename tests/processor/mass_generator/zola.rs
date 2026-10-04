//! The zola mass generator against the real zola. The fixture exercises
//! every content feature the planner models, and the tests assert that the
//! plan names exactly the files zola writes — the whole point of the
//! processor, and what a zola upgrade must be re-verified against.

use crate::common::{require_tool, run_rsconstruct_with_env, write_file};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

const SITE_CONFIG: &str = r#"base_url = "https://example.org"
default_language = "en"
title = "Fixture"
compile_sass = true
build_search_index = true
generate_feeds = true
taxonomies = [{ name = "tags", feed = true, paginate_by = 1 }]

[markdown.highlighting]
style = "class"
theme = "github-dark"

[languages.fr]
title = "Fixture FR"
generate_feeds = true
taxonomies = [{ name = "tags" }]

[slugify]
taxonomies = "safe"
"#;

const BASE_TEMPLATE: &str = "<html><body>{% block content %}{% endblock %}</body></html>\n";
const PAGE_TEMPLATE: &str =
    "{% extends \"base.html\" %}{% block content %}{{ page.title }}{% endblock %}\n";
const SECTION_TEMPLATE: &str = "{% extends \"base.html\" %}{% block content %}{{ section.title }}{% if paginator %}{{ paginator.current_index }}{% endif %}{% endblock %}\n";
const LIST_TEMPLATE: &str =
    "{% extends \"base.html\" %}{% block content %}{{ taxonomy.name }}{% endblock %}\n";
const TERM_TEMPLATE: &str =
    "{% extends \"base.html\" %}{% block content %}{{ term.name }}{% endblock %}\n";

fn page(title: &str, extra: &str) -> String {
    format!("+++\ntitle = \"{title}\"\n{extra}+++\nBody of {title}.\n")
}

fn section(extra: &str) -> String {
    format!("+++\ntitle = \"Section\"\n{extra}+++\n")
}

/// A site touching every path rule the planner implements.
fn setup_zola_site(config_extra: &str) -> TempDir {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let root = temp_dir.path();
    write_file(root, "config.toml", SITE_CONFIG);
    for (name, body) in [
        ("base.html", BASE_TEMPLATE),
        ("index.html", SECTION_TEMPLATE),
        ("section.html", SECTION_TEMPLATE),
        ("page.html", PAGE_TEMPLATE),
        ("taxonomy_list.html", LIST_TEMPLATE),
        ("taxonomy_single.html", TERM_TEMPLATE),
        ("404.html", BASE_TEMPLATE),
    ] {
        write_file(root, &format!("templates/{name}"), body);
    }
    write_file(
        root,
        "templates/data.html",
        "{% set d = load_data(path=\"static/data.toml\") %}\n",
    );
    write_file(root, "static/data.toml", "x = 1\n");
    write_file(root, "static/robots-extra.txt", "static file\n");
    write_file(root, "static/img/logo.svg", "<svg/>\n");
    write_file(
        root,
        "sass/style.scss",
        "@use 'vars';\nbody { color: vars.$c; }\n",
    );
    write_file(root, "sass/_vars.scss", "$c: red;\n");
    write_file(root, "sass/_partials/skip.scss", "a { b: c; }\n");

    // Root sections for both languages.
    write_file(root, "content/_index.md", &section(""));
    write_file(root, "content/_index.fr.md", &section(""));
    write_file(
        root,
        "content/top.md",
        &page("Top", "aliases = [\"old-top\"]\n"),
    );

    // A date-sorted, paginated blog with feeds: an undated page there is
    // not rendered at all, a hidden page renders but is not listed.
    write_file(
        root,
        "content/blog/_index.md",
        &section("sort_by = \"date\"\npaginate_by = 2\ngenerate_feeds = true\n"),
    );
    write_file(
        root,
        "content/blog/2024-01-02-dated-post.md",
        &page("Dated", "[taxonomies]\ntags = [\"Rust\", \"C++\"]\n"),
    );
    write_file(
        root,
        "content/blog/second.md",
        &page(
            "Second",
            "date = 2024-02-03\n[taxonomies]\ntags = [\"Rust\"]\n",
        ),
    );
    write_file(
        root,
        "content/blog/third.md",
        &page("Third", "date = 2024-03-04\nslug = \"custom slug\"\n"),
    );
    write_file(root, "content/blog/undated.md", &page("Undated", ""));
    write_file(
        root,
        "content/blog/hidden.md",
        &page("Hidden", "date = 2024-04-05\nhidden = true\n"),
    );
    write_file(
        root,
        "content/blog/not-rendered.md",
        &page("NotRendered", "date = 2024-05-06\nrender = false\n"),
    );
    write_file(
        root,
        "content/blog/draft.md",
        &page("Draft", "draft = true\n"),
    );
    write_file(
        root,
        "content/blog/second.fr.md",
        &page(
            "Deuxieme",
            "date = 2024-02-03\n[taxonomies]\ntags = [\"Rouille\"]\n",
        ),
    );

    // A colocated page with assets, and a section with its own asset.
    write_file(
        root,
        "content/blog/gallery/index.md",
        &page("Gallery", "date = 2024-06-07\n"),
    );
    write_file(root, "content/blog/gallery/photo.jpg", "jpg\n");
    write_file(root, "content/blog/gallery/sub/thumb.jpg", "jpg\n");
    write_file(root, "content/blog/banner.png", "png\n");

    // A transparent section passes its pages up; a section that does not
    // render still renders its pages; a draft section hides everything below.
    write_file(
        root,
        "content/blog/series/_index.md",
        &section("transparent = true\n"),
    );
    write_file(
        root,
        "content/blog/series/part-one.md",
        &page("PartOne", "date = 2024-07-08\npath = \"/parts/one\"\n"),
    );
    write_file(root, "content/docs/_index.md", &section("render = false\n"));
    write_file(root, "content/docs/guide.md", &page("Guide", ""));
    write_file(root, "content/wip/_index.md", &section("draft = true\n"));
    write_file(root, "content/wip/secret.md", &page("Secret", ""));

    // A page in a directory without an `_index.md` is an orphan, still rendered.
    write_file(root, "content/loose/orphan.md", &page("Orphan", ""));

    write_file(
        root,
        "rsconstruct.toml",
        &format!("[processor.mass_generator.zola]\noutput_dir = \"_site\"\n{config_extra}"),
    );
    temp_dir
}

fn files_below(dir: &Path) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        for entry in fs::read_dir(&current).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else {
                out.insert(
                    path.strip_prefix(dir)
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        }
    }
    out
}

/// The planned outputs, relative to `_site/`, read from the graph.
fn planned(root: &Path) -> BTreeSet<String> {
    let output = run_rsconstruct_with_env(
        root,
        &["graph", "show", "--format", "json"],
        &[("NO_COLOR", "1")],
    );
    assert!(
        output.status.success(),
        "graph show failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let graph: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    graph["products"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|p| p["outputs"].as_array().unwrap().clone())
        .map(|o| {
            o.as_str()
                .unwrap()
                .strip_prefix("_site/")
                .expect("every planned output lives in _site/")
                .to_string()
        })
        .collect()
}

#[test]
fn zola_plan_matches_what_zola_writes() {
    require_tool("zola");
    let temp_dir = setup_zola_site("");
    let root = temp_dir.path();

    let reference = root.join("reference");
    let status = std::process::Command::new("zola")
        .current_dir(root)
        .args(["build", "--output-dir", "reference"])
        .output()
        .unwrap();
    assert!(
        status.status.success(),
        "fixture must build with plain zola: {}",
        String::from_utf8_lossy(&status.stderr)
    );
    let written = files_below(&reference);
    fs::remove_dir_all(&reference).unwrap();

    let plan = planned(root);
    let missing: Vec<_> = written.difference(&plan).collect();
    let extra: Vec<_> = plan.difference(&written).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "plan and zola disagree\n  written by zola, not planned: {missing:?}\n  planned, not written: {extra:?}"
    );

    // The fixture really exercises the rules it is meant to.
    for expected in [
        "blog/dated-post/index.html",
        "blog/custom-slug/index.html",
        "blog/hidden/index.html",
        "blog/gallery/photo.jpg",
        "blog/gallery/sub/thumb.jpg",
        "blog/banner.png",
        "blog/page/1/index.html",
        "blog/page/2/index.html",
        "blog/atom.xml",
        "parts/one/index.html",
        "docs/guide/index.html",
        "loose/orphan/index.html",
        "old-top/index.html",
        "fr/blog/second/index.html",
        "fr/atom.xml",
        "tags/C++/index.html",
        "tags/Rust/page/2/index.html",
        "tags/Rust/atom.xml",
        "fr/tags/Rouille/index.html",
        "style.css",
        "giallo.css",
        "search_index.en.js",
        "elasticlunr.min.js",
        "img/logo.svg",
    ] {
        assert!(
            plan.contains(expected),
            "{expected} missing from plan: {plan:#?}"
        );
    }
    for absent in [
        "blog/undated/index.html",
        "blog/not-rendered/index.html",
        "blog/draft/index.html",
        "docs/index.html",
        "wip/secret/index.html",
        "_vars.css",
    ] {
        assert!(!plan.contains(absent), "{absent} must not be planned");
    }
}

#[test]
fn zola_build_restore_and_static_precision() {
    require_tool("zola");
    let temp_dir = setup_zola_site("");
    let root = temp_dir.path();
    let env = [("NO_COLOR", "1")];

    let build = run_rsconstruct_with_env(root, &["build"], &env);
    assert!(
        build.status.success(),
        "build failed: {}{}",
        String::from_utf8_lossy(&build.stdout),
        String::from_utf8_lossy(&build.stderr)
    );
    let built = files_below(&root.join("_site"));
    assert_eq!(built, planned(root), "the build writes exactly the plan");
    assert!(
        !root
            .join(".rsconstruct/mass_generator")
            .join("processor.mass_generator.zola.staging")
            .exists(),
        "staging is removed after a successful build"
    );

    // A static file is its own only source: editing it rebuilds one product.
    write_file(root, "static/img/logo.svg", "<svg>changed</svg>\n");
    let out = run_rsconstruct_with_env(root, &["--json", "build"], &env);
    let summary = String::from_utf8_lossy(&out.stdout);
    assert!(
        summary.contains("\"success\":1,"),
        "only the static product rebuilds: {summary}"
    );
    assert_eq!(
        fs::read_to_string(root.join("_site/img/logo.svg")).unwrap(),
        "<svg>changed</svg>\n"
    );

    // Everything restores from cache without zola.
    let clean = run_rsconstruct_with_env(root, &["clean", "outputs"], &env);
    assert!(clean.status.success());
    let restore = run_rsconstruct_with_env(root, &["build", "--show-child-processes"], &env);
    assert!(restore.status.success());
    assert!(
        !String::from_utf8_lossy(&restore.stderr).contains("zola build"),
        "a full restore must not run zola"
    );
    assert_eq!(files_below(&root.join("_site")), built);
}

/// A data file a template loads by a literal path is a source of every
/// rendered page, so editing it re-renders them.
#[test]
fn zola_load_data_file_is_a_rendered_source() {
    require_tool("zola");
    let temp_dir = setup_zola_site("");
    let root = temp_dir.path();
    let env = [("NO_COLOR", "1")];
    assert!(
        run_rsconstruct_with_env(root, &["build"], &env)
            .status
            .success()
    );

    write_file(root, "static/data.toml", "x = 2\n");
    let out = run_rsconstruct_with_env(root, &["--json", "build"], &env);
    assert!(out.status.success());
    // The build log names a product by its first input, so count instead:
    // the static copy of data.toml alone would be one rebuilt product; the
    // rendered pages that load it add dozens.
    let result = crate::common::BuildResult::parse(&out);
    assert!(
        result.success > 10,
        "rendered pages must rebuild when a loaded data file changes: {} rebuilt",
        result.success
    );
}

/// Other processors may write into the output directory; zola builds in a
/// private staging directory, so their files survive a zola run.
#[test]
fn zola_does_not_wipe_a_neighbor_in_the_output_dir() {
    require_tool("zola");
    let temp_dir = setup_zola_site(
        "\n[processor.explicit.generic.extra]\ncommand = \"./extra.sh\"\ninputs = [\"extra.txt\"]\noutput_files = [\"_site/extra.html\"]\n",
    );
    let root = temp_dir.path();
    write_file(
        root,
        "extra.sh",
        "#!/bin/bash\nset -e\nmkdir -p _site\necho extra > _site/extra.html\n",
    );
    crate::common::make_executable(&root.join("extra.sh"));
    write_file(root, "extra.txt", "x\n");
    let env = [("NO_COLOR", "1")];
    assert!(
        run_rsconstruct_with_env(root, &["build"], &env)
            .status
            .success()
    );

    // A content edit makes zola run again while the neighbor stays clean.
    write_file(
        root,
        "content/top.md",
        &page("Top", "aliases = [\"old-top\"]\n").replace("Body", "New body"),
    );
    let out = run_rsconstruct_with_env(root, &["build"], &env);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        fs::read_to_string(root.join("_site/extra.html")).unwrap(),
        "extra\n",
        "zola must not delete files other processors put in _site/"
    );
}

#[test]
fn zola_refuses_unmodeled_features() {
    require_tool("zola");
    let temp_dir = setup_zola_site("");
    let root = temp_dir.path();
    let config = fs::read_to_string(root.join("config.toml")).unwrap();
    write_file(
        root,
        "config.toml",
        &config.replace(
            "compile_sass = true",
            "compile_sass = true\nignored_static = [\"*.tmp\"]",
        ),
    );
    let out = run_rsconstruct_with_env(root, &["build"], &[("NO_COLOR", "1")]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("`ignored_static` is not supported by the zola processor"),
        "{stderr}"
    );

    write_file(root, "config.toml", &config);
    write_file(
        root,
        "templates/thumb.html",
        "{% set i = resize_image(path=\"x.jpg\", width=10, op=\"scale\") %}\n",
    );
    let out = run_rsconstruct_with_env(root, &["build"], &[("NO_COLOR", "1")]);
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("calls resize_image"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
