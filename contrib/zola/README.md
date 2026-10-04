# `zola plan` patch

`zola-plan-v0.23.3.patch` adds a `plan` subcommand to [zola](https://github.com/getzola/zola)
v0.23.3. `zola plan` loads the site exactly as `zola check` does and prints the
JSON manifest that rsconstruct's
[mass generator](../../docs/src/processor/mass_generator/generic.md) expects:
every file `zola build` would write, each with the input files whose content
can change it. Nothing is rendered or written.

It reuses zola's own job queue (`Queue::full_build`) and the same code paths
the build uses for Sass, search indexes, highlight themes, processed images
and the static copy, so the plan cannot drift from the build. Templates and
the config file are listed as sources of every rendered output (the whole
template tree, not the exact `extends`/`include` chain — always correct, and
zola rebuilds everything on any change anyway).

## Build

```bash
git clone --depth 1 --branch v0.23.3 https://github.com/getzola/zola.git
cd zola
git apply /path/to/rsconstruct/contrib/zola/zola-plan-v0.23.3.patch
cargo build --release
```

The patch applies cleanly to the `v0.23.3` tag (verified 2026-10-04). Against
another zola version, expect to adjust `components/site/src/queue.rs`: the new
`Queue::plan` mirrors `execute_job` job for job, and its `match` is exhaustive
so a new kind of job is a compile error rather than a silent gap in the plan.

## Use

```toml
[processor.mass_generator.generic.site]
command         = "zola"
args            = ["build", "--output-dir", "_site", "--force"]
predict_command = "zola"
predict_args    = ["plan", "--output-dir", "_site"]
output_dirs     = ["_site"]
```

Run from the project root, with the same `--output-dir` for both commands;
`zola plan` reports paths relative to the current directory.

## Verified against a real site

On `veltzer.github.io` (800 pages, 20 sections, two languages) the plan listed
exactly the 1,946 files `zola build` wrote, in 0.8 s. Under rsconstruct: an
unchanged site ran zola zero times, an edited post rebuilt 56 products with
one zola run, and `clean outputs` followed by a build restored all 1,946 files
from cache without running zola.

## Known gaps

- An image resized from a *template* (rather than from markdown) is enqueued
  only while that template renders, so it is not in the plan; the mass
  generator's plan-vs-build check reports it.
- One `sitemap.xml` is assumed; zola splits the sitemap above 30,000 entries.

## Upstreaming

The patch is shaped as an upstream PR (new `components/site/src/plan.rs`,
`src/cmd/plan.rs`, small hooks in `queue.rs`, `lib.rs`, `sass.rs` and
`imageproc`). It has not been submitted.
