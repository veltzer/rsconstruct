//! The sphinx mass generator against the real Sphinx. The fixture turns on
//! the extensions that add files beyond one page per document (autodoc,
//! viewcode, githubpages), a static path and an image, and the tests assert
//! that the plan names exactly the files `sphinx-build` writes — the whole
//! point of the processor — and that the products then behave like any
//! other generator's: rebuilt, restored and extended as sources change.

use crate::common::{
    BuildResult, require_tool, run_rsconstruct_json_with_env, run_rsconstruct_with_env, write_file,
};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

const CONF_PY: &str = "import os
import sys

sys.path.insert(0, os.path.abspath(\"../src\"))
project = \"Fixture\"
extensions = [
    \"sphinx.ext.autodoc\",
    \"sphinx.ext.viewcode\",
    \"sphinx.ext.githubpages\",
]
html_static_path = [\"_static\"]
";
const INDEX_RST: &str = "Fixture\n=======\n\n.. image:: pic.svg\n\n.. toctree::\n\n   api\n";
const API_RST: &str = "API\n===\n\n.. automodule:: fixturepkg.core\n   :members:\n";
const CORE_PY: &str = "def add(a, b):\n    \"\"\"Add two numbers.\"\"\"\n    return a + b\n";
const CONFIG: &str = "[processor.mass_generator.sphinx]\ndep_inputs = [\"src/fixturepkg/*.py\"]\n";

fn setup(config: &str) -> TempDir {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let root = temp_dir.path();
    write_file(root, "sphinx/conf.py", CONF_PY);
    write_file(root, "sphinx/index.rst", INDEX_RST);
    write_file(root, "sphinx/api.rst", API_RST);
    write_file(
        root,
        "sphinx/pic.svg",
        "<svg xmlns=\"http://www.w3.org/2000/svg\"/>\n",
    );
    write_file(root, "sphinx/_static/custom.css", "body { color: red }\n");
    write_file(root, "src/fixturepkg/__init__.py", "");
    write_file(root, "src/fixturepkg/core.py", CORE_PY);
    write_file(root, "rsconstruct.toml", config);
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

/// The planned outputs, relative to `docs/`, read from the graph.
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
                .strip_prefix("docs/")
                .expect("every planned output lives in docs/")
                .to_string()
        })
        .collect()
}

fn build_ok(root: &Path) -> BuildResult {
    let result = run_rsconstruct_json_with_env(root, &["build"], &[("NO_COLOR", "1")]);
    assert!(result.exit_success, "build failed: {:?}", result.products);
    result
}

/// Staging, doctree and planner scratch directories are gone once a run is
/// over; only the plan cache stays.
fn assert_no_scratch_left(root: &Path) {
    let dir = root.join(".rsconstruct/mass_generator");
    let left: Vec<String> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| !name.ends_with(".json"))
        .collect();
    assert!(left.is_empty(), "scratch directories left behind: {left:?}");
}

#[test]
fn sphinx_plan_matches_what_sphinx_writes() {
    require_tool("sphinx-build");
    let temp_dir = setup(CONFIG);
    let root = temp_dir.path();

    let status = std::process::Command::new("sphinx-build")
        .current_dir(root)
        .args([
            "-q",
            "-b",
            "html",
            "-E",
            "-a",
            "-d",
            "reference-doctrees",
            "sphinx",
            "reference",
        ])
        .output()
        .unwrap();
    assert!(
        status.status.success(),
        "fixture must build with plain sphinx-build: {}",
        String::from_utf8_lossy(&status.stderr)
    );
    let written = files_below(&root.join("reference"));
    fs::remove_dir_all(root.join("reference")).unwrap();
    fs::remove_dir_all(root.join("reference-doctrees")).unwrap();

    let plan = planned(root);
    let missing: Vec<_> = written.difference(&plan).collect();
    let extra: Vec<_> = plan.difference(&written).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "plan and sphinx-build disagree\n  written by sphinx-build, not planned: {missing:?}\n  planned, not written: {extra:?}"
    );

    // The fixture really exercises the rules it is meant to.
    for expected in [
        "index.html",
        "api.html",
        "_sources/api.rst.txt",
        "_modules/index.html",
        "_modules/fixturepkg/core.html",
        "py-modindex.html",
        "genindex.html",
        "search.html",
        "searchindex.js",
        "objects.inv",
        ".buildinfo",
        ".nojekyll",
        "_images/pic.svg",
        "_static/custom.css",
        "_static/basic.css",
    ] {
        assert!(
            plan.contains(expected),
            "{expected} missing from the plan: {plan:?}"
        );
    }
    assert_no_scratch_left(root);
}

#[test]
fn sphinx_products_build_restore_and_follow_their_sources() {
    require_tool("sphinx-build");
    let temp_dir = setup(CONFIG);
    let root = temp_dir.path();
    let count = planned(root).len();

    let result = build_ok(root);
    assert_eq!(
        result.count_status("success"),
        count,
        "{:?}",
        result.products
    );
    assert!(root.join("docs/index.html").is_file());
    assert!(root.join("docs/_modules/fixturepkg/core.html").is_file());
    assert!(root.join("docs/.nojekyll").is_file());
    assert!(
        !root.join("docs/.doctrees").exists(),
        "doctrees stay out of the output directory"
    );
    assert_no_scratch_left(root);

    // Nothing changed: nothing runs.
    let again = build_ok(root);
    assert_eq!(again.count_status("success"), 0, "{:?}", again.products);

    // After `clean outputs` every file comes back from cache, without Sphinx.
    let clean = run_rsconstruct_with_env(root, &["clean", "outputs"], &[("NO_COLOR", "1")]);
    assert!(
        clean.status.success(),
        "{}",
        String::from_utf8_lossy(&clean.stderr)
    );
    assert!(!root.join("docs/index.html").exists());
    let restored = build_ok(root);
    assert_eq!(restored.restored, count, "{:?}", restored.products);
    assert_eq!(
        restored.count_status("success"),
        0,
        "{:?}",
        restored.products
    );
    assert!(root.join("docs/_static/custom.css").is_file());

    // A docstring in the package (a dep_input) is a source of every page.
    write_file(
        root,
        "src/fixturepkg/core.py",
        "def add(a, b):\n    \"\"\"Add two numbers.\"\"\"\n    return a + b\n\n\ndef subtract(a, b):\n    \"\"\"Subtract the second number.\"\"\"\n    return a - b\n",
    );
    let changed = build_ok(root);
    assert_eq!(
        changed.count_status("success"),
        count,
        "{:?}",
        changed.products
    );
    assert!(
        fs::read_to_string(root.join("docs/api.html"))
            .unwrap()
            .contains("Subtract the second number"),
        "api.html must be Sphinx's fresh output, not the cached page"
    );

    // A new document extends the plan: its page and its source copy appear.
    write_file(root, "sphinx/extra.rst", "Extra\n=====\n\nMore.\n");
    write_file(
        root,
        "sphinx/index.rst",
        "Fixture\n=======\n\n.. image:: pic.svg\n\n.. toctree::\n\n   api\n   extra\n",
    );
    let extended = build_ok(root);
    assert_eq!(
        extended.count_status("success"),
        count + 2,
        "{:?}",
        extended.products
    );
    assert!(root.join("docs/extra.html").is_file());
    assert!(root.join("docs/_sources/extra.rst.txt").is_file());
    assert_no_scratch_left(root);
}

#[test]
fn sphinx_refuses_the_arguments_it_owns() {
    let temp_dir =
        setup("[processor.mass_generator.sphinx]\nargs = [\"-W\", \"-Dproject=Other\"]\n");
    let output = run_rsconstruct_with_env(temp_dir.path(), &["build"], &[("NO_COLOR", "1")]);
    assert!(!output.status.success(), "a -D override must be refused");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("args entry '-Dproject=Other' is not allowed"),
        "{stderr}"
    );
}
