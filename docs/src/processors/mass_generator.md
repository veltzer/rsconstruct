# MassGenerator Processor

## Status

**Designed, not yet implemented.** This document describes the intended user-facing contract for the MassGenerator processor type. The full design rationale is in [Output Prediction](../output-prediction.md).

## Why "mass generator"?

Existing processor types cover a matrix of "how many outputs" and "are they known in advance":

| Type             | Outputs known?              | Example                |
|------------------|-----------------------------|------------------------|
| **Generator**    | Yes, 1 per input            | tera: template → file  |
| **Explicit**     | Yes, user-declared          | custom build step      |
| **Checker**      | None (pass/fail)            | ruff                   |
| **Creator**      | No, opaque (`output_dirs`)  | mkdocs → `_site/`      |
| **MassGenerator**| Yes — tool enumerates them  | rssite → `_site/*`     |

MassGenerator is the "transparent Creator": it produces many output files (like a Creator), but the tool itself answers the question *"what will you produce?"* before running. Each predicted file becomes a declared product with its own inputs, cache entry, and dependency edges.

Names considered:

- **mass_generator** — chosen. Says what it does: "generator" (per-file outputs like the Generator type), "mass" (many products from one tool invocation).
- **transparent_creator** — accurate but awkward.
- **predicting_creator** — describes the mechanism, not the result.
- **site_generator** — too narrow; the type is useful beyond static sites.

## Purpose

Wraps a tool that:

1. Produces many output files from a set of source files (e.g., a static site generator).
2. Can enumerate its outputs in advance via a separate "plan" command.
3. Normally builds all its outputs in a single invocation.

Once wired as a MassGenerator, the tool gets per-file cache entries, plays cleanly with other processors sharing its output directory, and allows downstream processors to depend on its outputs.

## How it works

### 1. The tool provides two modes

The wrapped tool must expose:

- **Build mode**: runs the actual generation. Produces all output files in one invocation.
- **Plan mode**: prints a JSON manifest to stdout listing every output it will produce, with per-output source dependencies. Does not produce any output files.

Both modes must be driven by the same internal function that enumerates outputs — otherwise the plan and the build diverge, and the cache is corrupted. This is a discipline the tool author upholds.

### 2. Plan phase (at graph-build time)

rsconstruct runs `predict_command` and parses its output. For each entry in the manifest, a product is added to the build graph with:

- `inputs` = the entry's `sources` (files whose changes should trigger this output's rebuild)
- `outputs` = `[entry.path]`
- `processor` = the MassGenerator instance name

### 3. Build phase

rsconstruct groups all dirty products for a MassGenerator instance into a single batch. The tool's `command` is invoked once per batch; it produces all predicted files. Each product caches its own file as a blob, independently of the others.

In **strict mode** (default), after the tool exits rsconstruct verifies that every predicted file was produced and no unexpected files appeared in `output_dirs`. Mismatches are build-breaking errors.

### 4. Restore phase

When all products for a MassGenerator instance are clean, each is restored from its blob cache — the tool is not invoked at all. Partial cleanliness (some products clean, some dirty) triggers a single tool invocation, and clean products are cached/re-cached afterward.

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

- `version` — integer. Schema version. Current: `1`.
- `outputs[].path` — relative path. Must fall within one of the processor's `output_dirs`.
- `outputs[].sources` — minimal set of input files whose changes invalidate this output.

## Configuration

```toml
[processor.mass_generator.site]
command         = "rssite build"
predict_command = "rssite plan"
output_dirs     = ["_site"]
src_dirs        = ["docs", "templates"]
src_extensions  = [".md", ".html", ".yaml"]
# loose_manifest = false   # optional; set to true to downgrade verification mismatches to warnings
```

### Fields

| Key               | Type                   | Required | Description                                                       |
|-------------------|------------------------|----------|-------------------------------------------------------------------|
| `command`         | string                 | yes      | Tool's build command. Invoked once per batch of dirty products.   |
| `predict_command` | string                 | yes      | Tool's plan command. Must print JSON manifest to stdout.          |
| `output_dirs`     | array of strings       | yes      | Directories the tool produces files in. Used for verification.    |
| `loose_manifest`  | bool                   | no       | Default false. If true, plan/actual mismatches are warnings only. |
| `src_dirs`        | array of strings       | no       | Bound which source changes trigger a replan.                      |
| `src_extensions`  | array of strings       | no       | As above.                                                         |
| `src_exclude_*`   | array of strings       | no       | Standard scan exclusions apply.                                   |
| `dep_inputs`      | array of strings       | no       | Extra files that invalidate the whole instance when changed.      |

## Cross-processor dependencies

Because every output file is a declared product, downstream processors wire up naturally:

```toml
[processor.mass_generator.site]
command         = "rssite build"
predict_command = "rssite plan"
output_dirs     = ["_site"]

[processor.markdownlint]
# Depends on rssite's outputs automatically via file-scan:
# any _site/*.html file is a discovered virtual file in the graph.
src_dirs       = ["_site"]
src_extensions = [".html"]
```

No ordering hacks needed. The graph's topological sort handles it.

## Tool author contract

For a tool to be compatible with MassGenerator, its plan command must uphold these invariants:

1. **Pure function of config + source tree.** Same inputs → same manifest, bit for bit. No network, no timestamps, no env-var peeking (unless declared as a source).
2. **Cheap or cached.** rsconstruct invokes it on every graph build. Slow plan → slow rsconstruct.
3. **Exact match with build output.** Predicted paths must equal actual paths produced by `command`. Violations are errors in strict mode.
4. **Deterministic variable outputs.** Content-derived outputs (tag pages, archive indices, RSS) must be enumerable from the same parsing pass that plan does.

See [rssite](https://github.com/veltzer/rssite) for a reference tool being built to this contract.

## Comparison with other processor types

|                           | Creator (opaque)        | MassGenerator (transparent)   | Generator (1:1)         |
|---------------------------|-------------------------|-------------------------------|-------------------------|
| Outputs known in advance? | No                      | Yes                           | Yes                     |
| Tool invocations per build | 1 if dirty              | 1 if any product is dirty     | N (one per dirty input) |
| Cache unit                | Whole tree              | Per file                      | Per file                |
| Downstream deps           | Only on declared files  | On every predicted file       | On every produced file  |
| Shared-folder safety      | Via `path_owner` filter | Via declared outputs (normal) | Via declared outputs    |
| Use case                  | mkdocs, Sphinx          | rssite, cooperative tools     | tera, mako, compilers   |

## Migration story

If a tool exists first as a Creator (`output_dirs` only) and later adds plan support, the migration is config-only:

```toml
# Before
[processor.creator.mysite]
command     = "mysite build"
output_dirs = ["_site"]

# After
[processor.mass_generator.mysite]
command         = "mysite build"
predict_command = "mysite plan"
output_dirs     = ["_site"]
```

No code changes; existing downstream processors start getting precise dependencies automatically.

## See also

- [Output Prediction](../output-prediction.md) — full design rationale, invariants, execution shape
- [Shared Output Directory](../shared-output-directory.md) — the fallback mechanism for opaque Creators
- [Processor Ordering](../processor-ordering.md) — sibling discussion about explicit ordering knobs
- [rssite](https://github.com/veltzer/rssite) — a static site generator being built to implement this contract
