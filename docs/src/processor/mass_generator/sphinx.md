# Sphinx Processor

## Purpose

Builds Sphinx HTML documentation with `sphinx-build`, and gives every file
Sphinx writes a product of its own: its own cache entry, its own dependency
edges, its own place in the graph for downstream processors.

It is the [mass generator](generic.md) contract with the plan supplied by
Sphinx itself. Sphinx has no plan mode, but it knows every file before it
writes one: after its read phase the environment holds the documents, the
domain indices, the pages extensions add and the theme's static files.

## How It Works

1. **Plan.** At graph-build time the processor runs a small planner with
   `python` — a program that drives Sphinx through its read phase and asset
   copying into a scratch directory under `.rsconstruct/`, never rendering a
   page, and prints the files the HTML builder would write: one page per
   document, `_sources/` copies, `genindex.html`, the domain indices
   (`py-modindex.html`), every page an extension contributes on
   `html-collect-pages` (viewcode's `_modules/`), `search.html`,
   `searchindex.js`, `objects.inv`, `.buildinfo`, `_static/`, `_images/`,
   `_downloads/`, `html_extra_path` files, and `.nojekyll` when
   `sphinx.ext.githubpages` is on. Each becomes a product. Nothing about
   Sphinx's layout is reimplemented in Rust.
2. **Build.** The first product that needs building runs
   `sphinx-build -b html -E -a -d <doctrees> <root> <staging>` into private
   directories under `.rsconstruct/`; the others find it done. rsconstruct
   compares what Sphinx wrote with the plan — exactly, both ways — and moves
   the planned files into the output directory.
3. **Restore.** When every product is clean, nothing runs; after
   `rsconstruct clean outputs`, every file is restored from cache without
   Sphinx.

The plan is cached in `.rsconstruct/mass_generator/<instance>.json`, keyed
on the instance config and the content of every file under `root` and in
`dep_inputs`; the planner runs again only when one of them changes.

## What a rendered file depends on

A Sphinx page can change when any other source changes: a toctree lists the
other documents' titles, cross-references resolve against every document,
the general index and the search index cover all of them, and autodoc pages
render the package's docstrings. Every product therefore lists as its
sources every file under `root` plus `dep_inputs`, and Sphinx runs once per
build whenever any of them changed.

Name in `dep_inputs` what Sphinx reads outside `root`: the package autodoc
imports, and whatever `conf.py` reads (a `pyproject.toml` it takes the
version from, the lock file that pins the Sphinx version — a Sphinx upgrade
changes `_static/` and every page).

## Arguments the processor owns

`args` goes to `sphinx-build` as given, except that the processor refuses
the options it sets itself or that would change the configuration behind the
planner's back: `-b`/`-M` (the builder is always `html`), `-E`, `-a`, `-d`,
`-D`, `-A`, `-t`, `-c` and `-C`. Configuration lives in `conf.py`, which the
planner and the build both read. `-W`, `-n`, `-q`, `-j` and the like pass
through.

A disagreement between plan and build — an extension writing a file the
planner does not know about — fails the build with the exact list of missing
and unexpected files, and leaves the staging directory in place for
inspection.

## Source Files

- Input: the Sphinx source directory (`conf.py`, the documents, `_static/`,
  `_templates/`) and `dep_inputs`
- Output: every file `sphinx-build -b html` writes, under `output_dir`

## Configuration

```toml
[processor.mass_generator.sphinx]
# root = "sphinx"          # The source directory holding conf.py
# output_dir = "docs"      # Where the HTML goes
dep_inputs = ["src/mypkg/**/*.py", "pyproject.toml", "uv.lock"]
# args = ["-W"]            # Extra sphinx-build arguments
# python = "python3"       # The interpreter that runs the planner
# loose_manifest = false   # true: report plan/build mismatches as warnings
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `command` | string | `"sphinx-build"` | The sphinx-build executable |
| `args` | string[] | `[]` | Extra arguments for `sphinx-build` (see the owned options above) |
| `root` | string | `"sphinx"` | The Sphinx source directory, relative to the project root |
| `output_dir` | string | `"docs"` | Where the HTML is written |
| `python` | string | `"python3"` | The interpreter the planner runs with; it must import the same Sphinx as `command` |
| `dep_inputs` | string[] | `[]` | Extra sources of every product (the autodoc'd package, files `conf.py` reads) |
| `loose_manifest` | boolean | `false` | Report plan/build mismatches as warnings instead of errors |

Both `command` and `python` resolve from the active environment, the
repository's own venv; the processor never switches environments itself.

## Batch support

Sphinx runs once per build, for all products together.

## Clean behavior

This processor is a mass generator — `rsconstruct clean outputs` removes each planned output file individually with no directory recursion. After all per-product cleans complete, the orchestrator removes any parent directories that are now empty. Pass `--no-empty-dirs` to keep them. See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).

A file Sphinx stops writing (a deleted document) has no product and is
neither rebuilt nor cleaned; delete it by hand.

## See also

- [Mass Generator](generic.md) — the generic, `predict_command`-driven form of the same contract
- [Zola](zola.md) — the other builtin mass generator, with its plan computed in Rust
