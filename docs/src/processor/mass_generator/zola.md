# Zola Processor

## Purpose

Builds a [zola](https://www.getzola.org) site with the official zola binary,
and gives every file zola writes a product of its own: its own cache entry,
its own dependency edges, its own place in the graph for downstream
processors.

Zola has no plan mode, so this processor computes the list of files zola will
write itself, by reading `config.toml` and the content front matter the way
zola 0.23.3 does. It is the [mass generator](generic.md) contract, with the
plan computed natively instead of by a `predict_command`.

## How It Works

1. **Plan.** At graph-build time the processor reads the site and lists every
   output: pages (slug, `slug`/`path` overrides, dates in file names, language
   suffixes), sections, pagination, taxonomy lists and terms (with their
   pagination and feeds), site and section feeds, aliases, colocated and
   section assets, `404.html`, `sitemap.xml`, `robots.txt`, the search index,
   highlighting stylesheets, compiled Sass, and the static tree. Each becomes
   a product.
2. **Build.** The first product that needs building runs
   `zola build --output-dir <staging> --force` into a private directory under
   `.rsconstruct/`; the others find it done. rsconstruct compares what zola
   wrote with the plan — exactly, both ways — and moves the planned files into
   the output directory.
3. **Restore.** When every product is clean, nothing runs; after
   `rsconstruct clean outputs`, every file is restored from cache without
   zola.

zola builds into staging rather than into the output directory because
`zola build` deletes its output directory first: building into `_site/`
directly would delete the files other processors put there.

## What a rendered file depends on

A zola template can read any page or section (`get_page`, `get_section`,
sibling links, taxonomy listings, the paginator), so a rendered file's
content can change when *another* file changes. Every rendered output
therefore lists as its sources the whole `content/` tree, every template, the
config file, and every data file a template or content file loads with a
literal `load_data(path = "...")`. A file loaded through a computed path is
invisible to the planner; list it in `dep_inputs`.

Static files and compiled Sass keep precise sources (the file itself, the
`sass/` tree), so editing a static file rebuilds one product.

zola runs once per build whenever anything it reads changed, regardless of
how many products are dirty.

## What is not supported

The planner refuses, by name, what it cannot predict, instead of guessing:

- a zola other than 0.23.3 (`zola --version` is checked on every plan; a zola
  upgrade means re-verifying the planner, which the fixture test does);
- `theme`, `ignored_content`, `ignored_static`, a `[languages]` table for the
  default language;
- `resize_image` in a template or content file (processed images are only
  known by rendering);
- sites rendering 30,000 or more HTML files (zola then splits the sitemap).

A disagreement that slips through anyway — a rule the planner gets wrong —
fails the build with the exact list of missing and unexpected files, and
leaves the staging directory in place for inspection.

## Source Files

- Input: the zola site (`config.toml`/`zola.toml`, `content/`, `templates/`,
  `sass/`, `static/`)
- Output: every file `zola build` writes, under the output directory

## Configuration

```toml
[processor.mass_generator.zola]
output_dir = "_site"       # Default: the site config's output_dir ("public")
# root = "site"            # The site directory, if not the project root
# dep_inputs = ["data/computed.toml"]
# loose_manifest = false   # true: report plan/build mismatches as warnings
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `command` | string | `"zola"` | The zola binary (must be 0.23.3) |
| `args` | string[] | `[]` | Extra arguments for `zola build` |
| `root` | string | `""` | The site directory, relative to the project root |
| `output_dir` | string | `""` | Where the site is written; empty means the site config's `output_dir` |
| `dep_inputs` | string[] | `[]` | Extra sources of every product (data files loaded through computed paths) |
| `loose_manifest` | boolean | `false` | Report plan/build mismatches as warnings instead of errors |

## Batch support

zola runs once per build, for all products together.

## Clean behavior

This processor is a mass generator — `rsconstruct clean outputs` removes each planned output file individually with no directory recursion. After all per-product cleans complete, the orchestrator removes any parent directories that are now empty. Pass `--no-empty-dirs` to keep them. See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).

A file zola stops writing (a deleted page) has no product and is neither rebuilt nor cleaned; delete it by hand.

## See also

- [Mass Generator](generic.md) — the generic, `predict_command`-driven form of the same contract
- [`zola plan` patch](https://github.com/veltzer/rsconstruct/tree/master/contrib/zola) — the alternative: a plan mode inside zola itself, not used by this processor
