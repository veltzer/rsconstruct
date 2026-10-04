# Mass Generator Processor

Runs a tool that enumerates its outputs in advance. The tool's plan command prints a manifest; rsconstruct turns every entry into a product and runs the tool's build command at most once per build. The design rationale is in [Output Prediction](../../internal/output-prediction.md).

## Why "mass generator"?

Existing processor types cover a matrix of "how many outputs" and "are they known in advance":

| Type               | Outputs known?              | Example                |
|--------------------|-----------------------------|------------------------|
| **Generator**      | Yes, 1 per input            | tera: template → file  |
| **Explicit**       | Yes, user-declared          | custom build step      |
| **Checker**        | None (pass/fail)            | ruff                   |
| **Creator**        | No, opaque (`output_dirs`)  | mkdocs → `_site/`      |
| **Mass generator** | Yes — tool enumerates them  | rssite → `_site/*`     |

A mass generator is the transparent creator: it produces many output files (like a creator), but the tool itself answers *"what will you produce?"* before running. Each predicted file becomes a declared product with its own inputs, cache entry, and dependency edges.

Names considered:

- **mass_generator** — chosen. Says what it does: "generator" (per-file outputs like the generator type), "mass" (many products from one tool invocation).
- **transparent_creator** — accurate but awkward.
- **predicting_creator** — describes the mechanism, not the result.
- **site_generator** — too narrow; the type is useful beyond static sites.

## Purpose

Wraps a tool that:

1. Produces many output files from a set of source files (a static site generator, say).
2. Can enumerate its outputs in advance via a separate plan command.
3. Builds all its outputs in a single invocation.

Once wired as a mass generator, the tool gets a cache entry per file, shares its output directory safely with other processors, and lets downstream processors depend on individual outputs.

## Configuration

```toml
[processor.mass_generator.site]
command         = "rssite"
args            = ["build"]
predict_command = "rssite"
predict_args    = ["plan"]
output_dirs     = ["_site"]
# loose_manifest = true     # report plan/build mismatches as warnings; default false
# dep_inputs = ["mysite.toml"]
```

Commands are executed directly, never through a shell (see [No-Shell Policy](../../internal/no-shell-policy.md)), so a command and its arguments are separate fields: `command` + `args` for the build, `predict_command` + `predict_args` for the plan. They may be the same binary with different arguments or two separate scripts.

### Fields

| Key               | Type             | Required | Description                                                                 |
|-------------------|------------------|----------|-----------------------------------------------------------------------------|
| `command`         | string           | yes      | The tool's build command. Runs at most once per build.                      |
| `args`            | array of strings | no       | Arguments for `command`.                                                    |
| `predict_command` | string           | yes      | The tool's plan command. Must print the JSON manifest to stdout.            |
| `predict_args`    | array of strings | no       | Arguments for `predict_command`.                                            |
| `output_dirs`     | array of strings | yes      | Directories the tool writes into. Every manifest path must fall inside one. |
| `loose_manifest`  | bool             | no       | Default false. If true, plan/build mismatches are warnings, not errors.     |
| `dep_inputs`      | array of strings | no       | Extra files added to the inputs of every product.                           |
| `required_tools`  | array of strings | no       | Tools the commands shell out to, for `tools install` and version locking.   |

The scan fields (`src_dirs`, `src_extensions`, `src_files`) are rejected: the manifest's `sources` decide every product's inputs, so a scan field would be read by nobody. Someone migrating a creator config must drop its anchor-file scan.

## Manifest format

```json
{
  "version": 1,
  "outputs": [
    {
      "path": "_site/index.html",
      "sources": ["docs/index.md", "templates/default.html", "mysite.toml"]
    },
    {
      "path": "_site/about/index.html",
      "sources": ["docs/about.md", "templates/default.html", "mysite.toml"]
    }
  ]
}
```

- `version` — integer. Schema version; only `1` is accepted.
- `outputs[].path` — plain relative path (no leading `/`, no `.` or `..` components). Must fall within one of the processor's `output_dirs`. Listed once.
- `outputs[].sources` — the files whose content determines this output. At least one; the tool's own config file counts. These become the product's inputs, so keep the list minimal: an output that names every source rebuilds whenever any source changes.

Unknown keys are rejected. rsconstruct sorts entries by path, so the tool's output order does not matter.

Several outputs may list exactly the same sources — a tag's index page, its `page/1/` redirect and its feed all depend on the same posts. Each is still cached under its own path: the predicted path is part of the product's cache key, where for other processor types the output path follows from the input and needs no such entry.

## How it works

### Plan phase (at graph-build time)

rsconstruct runs `predict_command` once per instance and parses its stdout. A non-zero exit, malformed JSON, an unsupported version, or a path outside `output_dirs` fails the graph build with the tool's own message. For each manifest entry a product is added:

- `inputs` = the entry's `sources` (plus `dep_inputs`)
- `outputs` = `[entry.path]`
- `processor` = this instance

Predicted outputs join the file index as virtual files, so a downstream processor can discover them even before they exist on disk.

### Build phase

Products are classified individually, like any generator's: unchanged inputs skip, a known input checksum restores from the blob cache, anything else builds. The first product that needs building runs `command`; every later one in the same build finds the tool already run and does nothing but cache its file. If the tool fails, every remaining product of the instance fails with the same message rather than re-running a build that just failed.

Before the tool runs, rsconstruct unlinks every predicted path, including the ones whose products are clean. A clean file may be a read-only hardlink left by an earlier cache restore, which the tool could not overwrite. The tool then regenerates everything; the clean products keep their cache entries, so the next build matches them again as long as the tool is deterministic (same inputs, same bytes).

### Verification

After the tool exits, rsconstruct compares what it wrote against the plan:

- **missing** — a predicted path that does not exist afterwards.
- **unexpected** — a file under `output_dirs` that is new or was rewritten during the run, is not in the plan, and is not another product's declared output.

Either is a build error. Files the tool left untouched are not its business and are not reported: a neighbor processor's output, a `CNAME` committed into `_site/`, or a page from an older plan that nobody cleans up. A file another processor declares (an `explicit` writing `_site/about.html`, say) is never reported even if the tool rewrote it — that is the shared-directory contract of [Shared Output Directory](../../internal/shared-output-directory.md).

`loose_manifest = true` turns both findings into a warning on stderr. Even then a product whose own predicted file is absent fails: there is nothing to cache.

### Restore phase

When every product of an instance is cache-clean, each is restored from its blob independently and the tool does not run at all. `rsconstruct clean outputs` followed by `rsconstruct build` therefore never invokes the tool.

## Clean behavior

This processor is a mass generator — `rsconstruct clean outputs` removes each predicted output file individually with no directory recursion, exactly as for a generator. After all per-product cleans complete, the orchestrator removes any parent directories that are now empty. Pass `--no-empty-dirs` to keep them. See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).

A page the tool no longer predicts has no product and is neither rebuilt nor cleaned; delete it by hand.

## Cross-processor dependencies

Every output file is a declared product, so downstream processors wire up through the normal graph:

```toml
[processor.mass_generator.site]
command         = "rssite"
args            = ["build"]
predict_command = "rssite"
predict_args    = ["plan"]
output_dirs     = ["_site"]

[processor.explicit.sitemap]
command      = "./sitemap.sh"
inputs       = ["_site/index.html", "_site/about/index.html"]
output_files = ["_site/sitemap.xml"]
```

The sitemap runs after the site is built and rebuilds when either page changes. No ordering knobs are involved; the topological sort handles it.

## Tool author contract

For a tool to work as a mass generator, its plan command must uphold these invariants:

1. **Pure function of config + source tree.** Same inputs → same manifest, bit for bit. No network, no timestamps, no environment peeking (unless the file read is declared as a source).
2. **Cheap.** rsconstruct runs it at every graph build — `build`, `status`, `clean outputs`, `graph`. There is no manifest cache yet.
3. **Exact match with the build.** Predicted paths equal the paths `command` writes, no more and no fewer. Violations are errors.
4. **Deterministic build.** Same inputs → same bytes, or clean products will mismatch their cache after every rebuild of a neighbor.
5. **Deterministic variable outputs.** Content-derived pages (tag indexes, archives, feeds) must be enumerable from the same parsing pass that plan does.

Both modes should be driven by the same internal function that enumerates outputs — otherwise the plan and the build drift apart, and verification turns every build red.

See [rssite](https://github.com/veltzer/rssite) for a tool built to this contract.

## Comparison with other processor types

|                            | Creator (opaque)        | Mass generator (transparent)  | Generator (1:1)         |
|----------------------------|-------------------------|-------------------------------|-------------------------|
| Outputs known in advance?  | No                      | Yes                           | Yes                     |
| Tool invocations per build | 1 if dirty              | 1 if any product is dirty     | N (one per dirty input) |
| Cache unit                 | Whole tree              | Per file                      | Per file                |
| Downstream deps            | Only on declared files  | On every predicted file       | On every produced file  |
| Shared-folder safety       | Via `path_owner` filter | Via declared outputs (normal) | Via declared outputs    |
| Use case                   | mkdocs, Sphinx          | rssite, cooperative tools     | tera, mako, compilers   |

## Migration from a creator

If a tool exists first as a creator and later grows a plan mode, the migration is config-only:

```toml
# Before
[processor.creator.mysite]
command        = "mysite"
args           = ["build"]
src_extensions = ["mysite.toml"]
src_dirs       = ["."]
output_dirs    = ["_site"]

# After
[processor.mass_generator.mysite]
command         = "mysite"
args            = ["build"]
predict_command = "mysite"
predict_args    = ["plan"]
output_dirs     = ["_site"]
```

The anchor-file scan goes away: the manifest names the inputs. Downstream processors start getting per-file dependencies without changes on their side.

## See also

- [Output Prediction](../../internal/output-prediction.md) — design rationale, invariants, and what the implementation left for later
- [Shared Output Directory](../../internal/shared-output-directory.md) — the ownership rules for opaque creators, which mass generators inherit
- [Processor Ordering](../../internal/processor-ordering.md) — sibling discussion about explicit ordering knobs
- [rssite](https://github.com/veltzer/rssite) — a static site generator built to the manifest contract
