# Output Prediction & MassGenerator

A Creator (mkdocs, Sphinx, Jekyll, Hugo, etc.) declares `output_dirs = ["_site"]` — "I produce something in here, don't ask me what until I've run." This chapter specifies a new processor type, **MassGenerator**, that makes those tools **transparent**: the tool is asked in advance what it will produce, and each planned file is promoted to a declared product output.

Once outputs are known up front, per-file caching, precise incremental rebuilds, cross-processor dependencies on generated files, and safe output-conflict detection all come for free.

## Status

**Designed, not yet implemented.** This document is the design spec that guides the implementation.

Related designs:

- [Shared Output Directory](shared-output-directory.md) — the fallback mechanism for tools that can't predict outputs.
- [Processor Ordering](processor-ordering.md) — the sibling design discussion about explicit ordering knobs.

## The core idea

Today we treat tools like mkdocs as a black box:

```toml
[processor.creator.mkdocs]
command     = "mkdocs build --site-dir _site"
output_dirs = ["_site"]   # opaque — we only know the directory
```

The new approach asks the tool to emit a manifest before running:

```toml
[processor.mass_generator.mkdocs]
command         = "mkdocs build --site-dir _site"
predict_command = "mkdocs-plan"                  # prints a JSON manifest on stdout
output_dirs     = ["_site"]
```

rsconstruct invokes `predict_command` at graph-build time, parses its JSON output, and creates one product per planned file. Each product has its own `inputs` (taken from the manifest's `sources` field) and a single `outputs` entry (the planned path). From that point on, the product is a regular per-file Generator — caching, dependency tracking, and cross-processor wiring all work uniformly.

## Manifest format

`predict_command` must print a single JSON document to stdout in this shape:

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
    },
    {
      "path": "_site/assets/style.css",
      "sources": ["assets/style.scss", "assets/_vars.scss"]
    }
  ]
}
```

- **`version`** — integer. Schema version (1 for now). Allows future evolution without breaking existing tools.
- **`outputs`** — array, one entry per file the tool will produce.
- **`outputs[].path`** — output file path relative to the project root. Must fall within one of the processor's `output_dirs` (enforced).
- **`outputs[].sources`** — array of input paths whose changes should trigger rebuilding this output. Used as the product's `inputs`, which feed into cache-key computation.

Order within `outputs` must be deterministic (sorted by path). The `sources` array should be minimal — only the files whose content genuinely affects this specific output.

## Lifecycle

### 1. Plan phase (at graph-build time)

Once per MassGenerator instance declared in `rsconstruct.toml`:

1. Run `predict_command`. Capture stdout and exit status.
2. Exit status non-zero → fail the graph build with the tool's stderr in the error message.
3. Parse stdout as JSON. Malformed → fail the graph build.
4. Reject manifest if any `outputs[].path` falls outside the declared `output_dirs`.
5. For each manifest entry, add one product to the build graph:
   - `inputs` = entry's `sources`
   - `outputs` = [entry's `path`]
   - `processor` = this instance's name
6. Cache the manifest itself in the object store, keyed on a hash of `(config + input_checksum_of(source_tree))`. Re-planning is skipped when the hash matches.

The plan phase runs BEFORE the existing product-discovery phase, so predicted outputs are known to all downstream processors (linters, compressors, etc.) via the normal file-index/cross-processor-dependency mechanisms.

### 2. Build phase

When one or more MassGenerator products are dirty:

1. rsconstruct groups all dirty products belonging to the same MassGenerator instance into a **single execution batch**.
2. It invokes `command` exactly once per batch (not per product).
3. The tool produces all its output files in that one invocation.
4. Each product caches its own output file as a blob descriptor, independently.
5. In **strict mode** (default): after the tool exits, rsconstruct verifies that every predicted file in the batch was produced and no unexpected files appeared in `output_dirs`. A mismatch fails the build.
6. In **loose mode** (`--loose-manifest` CLI flag): divergence is a warning only.

The "one invocation, many products" idiom is this type's defining execution shape — distinct from both Generator (one invocation per product) and Creator (one invocation, one product).

### 3. Restore phase

When all MassGenerator products for an instance are cache-clean:

1. Each product is restored from its blob descriptor independently — no tool invocation at all.
2. Partial restoration is natural: if 47 of 50 files are clean, only 3 products go through the build phase (which still triggers one tool invocation, but the 47 unchanged files are either untouched on disk or silently overwritten with identical content).

### 4. Verification (strict mode)

After build:

- Every manifest entry → file exists with the right path.
- Every file in `output_dirs` → appears in the manifest OR belongs to another processor (via the existing `path_owner` query).

Violations are hard errors; partial output is left on disk for debugging.

## Graph shape

With a MassGenerator producing N planned files, the graph looks like this:

```
  source files (markdown, templates, config)
         |
         | (as inputs to each planned file's product)
         v
  [product: _site/index.html]
  [product: _site/about/index.html]
  [product: _site/assets/style.css]
  ... (N products, all with processor = "mass_generator.mkdocs")
```

Each product is a first-class citizen in the graph. A downstream linter can depend on `_site/index.html` like any other generated file.

## Execution: one tool invocation for many products

Today's executor assumes "one product = one invocation of `processor.execute(product)`." MassGenerator violates that. The cleanest implementation (per the design discussion) uses a two-level graph:

1. **Phase product** (internal, not user-visible): one synthetic product per MassGenerator instance whose `execute` is the actual tool invocation. It has no declared outputs; its job is to populate the output_dir.
2. **File products** (the N planned files): each depends on the phase product, meaning the tool must have run before any file product can be cached/restored. Each file product's `execute` is a no-op (tool already ran); it just caches its output.

The dependency system then naturally orders: phase product runs once (if any file product is dirty), then every dirty file product caches its output. Clean file products skip both phases.

This shape keeps the executor simple and reuses all existing caching, skipping, and restore logic without modification.

## Config reference

```toml
[processor.mass_generator.<INSTANCE>]

# The tool's build command. Runs once per batch of dirty file products.
command = "mkdocs build --site-dir _site"

# The tool's plan command. Must print the JSON manifest to stdout.
# May be the same binary with a different flag or a separate script.
predict_command = "mkdocs-plan"

# Where the tool will produce its outputs. Every manifest entry's path
# must fall inside one of these directories. Used for verification.
output_dirs = ["_site"]

# Standard scan fields still apply — they bound which source changes
# trigger a replan.
src_dirs = ["docs", "templates"]
src_extensions = [".md", ".html", ".yaml"]

# Optional: skip strict output verification for this instance.
# Useful during development of the tool itself. Default: false.
loose_manifest = false
```

## Interaction with the shared-output-directory design

This new processor type does not replace the Creator / shared-output-directory mechanism. Both coexist:

| User declares                   | Treated as       | Caching                    | Cross-processor deps    |
|---------------------------------|------------------|----------------------------|-------------------------|
| `output_dirs` only              | Creator (opaque) | One tree per build         | Only via declared files |
| `output_dirs` + `predict_command` | MassGenerator    | Per file                   | Full — all files known  |

Choose Creator when the tool can't enumerate its outputs. Choose MassGenerator when it can.

## Design invariants (for tool authors)

For a tool to be consumed as a MassGenerator, `predict_command` must uphold:

1. **Pure function of config + source tree.** Same inputs → same manifest, bit for bit.
2. **Cheap or cached.** rsconstruct calls this on every graph build. Slow predict_command means slow rsconstruct invocations.
3. **Matches the build command's actual outputs.** Predicted paths = actual paths. Violations are hard errors in strict mode.
4. **Deterministic variable outputs.** If the tool produces tag pages or archive pages or anything else content-derived, `predict_command` must compute them from the same source inspection pass.

The [rssite README](https://github.com/veltzer/rssite) spells out a concrete contract that meets these invariants.

## Advantages

### 1. Shared-directory ownership becomes trivial

Every generated file has a declared owner at graph-build time. The existing output-conflict check catches overlaps instantly:

> `Output conflict: _site/about.html is produced by both [mass_generator.mkdocs] and [explicit.pandoc]`

The complex `path_owner` + tree filtering + previous-tree cleanup mechanism (see [Shared Output Directory](shared-output-directory.md)) is still there as a safety net, but for MassGenerators it's mostly unnecessary.

### 2. True cross-processor dependencies

Downstream processors (linters, compressors, sitemap builders) can declare the MassGenerator's outputs as inputs. The graph connects properly. Impossible with opaque Creators.

### 3. Per-file caching

Change `docs/tutorial.md` → rebuild only `_site/tutorial.html`. On a large site this is the difference between "rebuild in 50ms" and "rebuild in 30s."

Note: the per-file caching on the rsconstruct side only saves the tool invocation when ALL file products are clean. If any one is dirty, the tool runs once and produces everything — then clean files are still cached individually (useful across different invocations). True per-file build speed requires the tool itself to support partial builds. rssite will; most existing tools won't.

### 4. Parallel file caching

With per-file products, different files can be cached to the object store in parallel after the build. Minor win, but free.

### 5. Precise clean, precise restore, real dep graphs

Every downstream feature that relies on declared outputs — `clean outputs <path>`, graph visualization, dry-run, watch mode — works correctly for MassGenerator outputs without special cases.

## Disadvantages

### 1. Predictor drift

If `predict_command` lies (or gets out of sync with the tool), the cache can be corrupted silently: predicted paths get restored, actual build produces different paths, orphan files accumulate. Strict-mode verification after each build is the guardrail — it catches drift at build time rather than at next-restore time.

### 2. Predict-time cost

Every graph build runs `predict_command`. For large sites this may mean parsing every source file to enumerate outputs. The manifest cache (keyed on source-tree hash) mitigates but doesn't eliminate this.

### 3. Partial build support

The per-product caching model wants "rebuild just this one file" but most tools rebuild everything per invocation. With `mkdocs`, `hugo`, `jekyll`, you pay full build cost whenever anything is dirty, regardless of how many files changed. rssite is being designed to support partial builds from day one; existing tools would need patches.

### 4. Engineering cost

The MassGenerator type is a new processor class with new execution semantics ("one invocation for many products"). That's real implementation work in the executor, plus a new config schema, plus manifest parsing, plus verification logic.

### 5. Variable outputs may require heavy parsing

Tag pages, archive indices, RSS feeds — all content-derived. `predict_command` has to do enough source parsing to enumerate them. For well-designed tools this is cheap (the same parsing feeds both plan and build). For retrofitted tools it's often duplicate work.

## Open questions

These should be resolved during implementation:

1. **Single-pass mode**: should we support a `--print-manifest` flag on `command` itself, so one invocation does both plan and build? Faster for full rebuilds, slightly uglier config. Probably yes, optional.
2. **Manifest schema evolution**: how do we handle `version: 2`? Support both for a transition period, or hard-require upgrade? Probably both-for-N-releases.
3. **Incremental invalidation**: when the manifest changes between builds (e.g., a new page added), how is the old cache cleaned? The existing descriptor-based cache handles this automatically (unreferenced cache entries are eventually pruned), but the behavior deserves explicit documentation.
4. **Interaction with `file_index`**: predicted outputs need to appear in the file index so downstream processors can discover them during their own scan phases. Must be registered before discover_products runs.
5. **Watch mode**: when a source file changes, do we re-run `predict_command` or reuse the last manifest? The hash-based cache mostly handles this, but edge cases around plugin-rewritten outputs need thinking.

## Recommendation

Build this once rssite (or any other cooperating tool) is far enough along to drive concrete requirements. Implementing it against a hypothetical tool wastes work — we'd guess at features. Implementing against rssite (where we control both sides) grounds the design in reality.

When implemented, do it in this order:

1. New processor type `mass_generator` registered in the plugin registry.
2. Config schema (`predict_command`, `loose_manifest`).
3. Plan phase: invoke `predict_command`, parse JSON, create products.
4. Execution phase: batching logic — one invocation per instance, per build.
5. Strict verification after build.
6. Manifest caching (skip re-plan when source tree unchanged).
7. Documentation in `docs/src/processors/mass_generator.md` once it's real.

## See also

- [MassGenerator processor type](processors/mass_generator.md) — processor-type documentation (forthcoming)
- [Shared Output Directory](shared-output-directory.md) — how we handle opaque Creators today
- [Processor Ordering](processor-ordering.md) — the sibling discussion about explicit ordering
- [Cross-Processor Dependencies](cross-processor-dependencies.md) — why per-file outputs enable proper dependency graphs
- [rssite](https://github.com/veltzer/rssite) — static site generator built to implement the MassGenerator contract
