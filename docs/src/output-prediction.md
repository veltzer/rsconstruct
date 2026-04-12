# Output Prediction for Mass Generators

A Creator (mkdocs, Sphinx, Jekyll, Hugo, etc.) declares `output_dirs = ["_site"]` — "I produce something in here, don't ask me what until I've run." This document explores an alternative design where we ask those tools *in advance* what they will produce, turning opaque Creators into transparent per-file generators.

## The idea

Today we treat tools like mkdocs as a black box:

```toml
[processor.creator.mkdocs]
command     = "mkdocs build --site-dir _site"
output_dirs = ["_site"]   # opaque — we only know the directory
```

The proposal: add a mechanism to discover the exact list of files the tool will produce *before* it runs. Two shapes:

### Shape A — Per-tool predictors built into rsconstruct

For each supported tool, write a Rust function:

```rust
fn predict_mkdocs_outputs(config: &Path, site_dir: &Path) -> Result<Vec<PathBuf>> {
    // Parse mkdocs.yml, walk docs/, apply use_directory_urls rule,
    // return ["_site/index.html", "_site/api/overview/index.html", ...]
}
```

Called at graph-build time. The predicted list becomes the product's declared `outputs`, and the Creator is promoted to a per-file Mass Generator.

### Shape B — Generic predict command

Add a config field that points to a script the user (or tool plugin) provides:

```toml
[processor.creator.mkdocs]
command        = "mkdocs build --site-dir _site"
predict_command = "./list-mkdocs-outputs.sh"
output_dirs    = ["_site"]
```

rsconstruct runs `predict_command` once per build, parses its stdout as a file list, treats those paths as declared `outputs`.

Shape B is the scalable form — it pushes tool-specific knowledge out of rsconstruct and into the user's/community's configuration. Shape A is the "batteries included" form for a few blessed tools.

## What prediction buys you

Once outputs are known in advance, the hard problems of opaque Creators evaporate into well-understood Generator territory.

### 1. Shared-directory ownership is trivial

The [Shared Output Directory](shared-output-directory.md) chapter explains a whole mechanism (path_owner queries, tree filtering, previous-tree-based cleanup) to handle the case where mkdocs and pandoc both write into `_site/`. With prediction, every file has a declared owner at graph-build time, and the existing output-conflict detection catches overlaps.

Collisions become graph errors instead of silent runtime bugs:

> `Output conflict: _site/about.html is produced by both [creator.mkdocs] and [explicit.pandoc]`

### 2. True cross-processor dependencies

Today, downstream processors cannot depend on a Creator's outputs because those outputs are unknown at scan time. If a linter wants to validate every `_site/*.html` file that mkdocs produces, it can't — those files don't appear in the `FileIndex` yet.

With prediction, `_site/index.html` etc. are known products. A linter (or ipdfunite, or an image optimizer) declares them as inputs, and the dependency graph connects correctly. See [Cross-Processor Dependencies](cross-processor-dependencies.md) for why this matters.

### 3. Per-file caching

A Creator's cache today is one big tree. Change any input → the whole tree is considered potentially dirty. With per-file products, each generated page has its own cache entry keyed by its own inputs. A docs fix in `docs/tutorial.md` rebuilds only `_site/tutorial.html`.

This is a **major** incremental-build win for sites with hundreds of pages.

### 4. Parallelism

Per-file products could in principle run in parallel — shard the work across cores. Whether this translates to actual parallelism depends on whether the underlying tool supports partial builds (most don't, see costs below).

### 5. Precise clean and restore

Cleaning a single stale page, restoring a subset from cache, showing a dependency graph with real edges — all of these become possible.

## What prediction costs

The idea is seductive; the costs are real.

### 1. Predictor drift

A predictor must exactly match the tool's actual behavior. When `mkdocs` changes its URL scheme in v2.0, introduces a new `use_directory_urls` mode, or ships plugins that rewrite output paths, our predictor lies. The cache restores the wrong paths and the actual build produces different ones — producing orphan files on disk and corrupting the cache.

Mitigating this requires either:
- Strict version pinning per tool (fragile, painful to maintain).
- A validation step that compares predicted vs. actual after each build, and errors on mismatch.
- Degrading gracefully to the current opaque Creator behavior when prediction fails.

None is free.

### 2. Plugin ecosystems

Most site generators have plugin ecosystems that rewrite outputs. mkdocs has ~50 plugins touching paths (navigation, search indices, social cards, i18n). Sphinx extensions add entirely new output formats. A predictor must either replicate plugin logic (impossibly fragile) or concede that prediction only works for plain vanilla configurations (limiting).

### 3. Partial build invocation

Even if we predict `_site/about.html` is stale and rebuild just that one file, most tools don't support partial builds:

- `mkdocs build` rebuilds the whole site each invocation.
- Sphinx has incremental builds but at the project level, not the file level.
- Hugo rebuilds everything in one pass by design.

So per-file parallelism buys nothing — one invocation still produces every file. The caching wins are real; the parallelism wins are mostly illusory for this class of tool.

### 4. What if prediction is wrong?

Two failure modes:

- **Under-prediction**: the actual run produces files we didn't predict. Are those files orphans? Do we add them to the cache retroactively? Error on "unexpected outputs"? Each answer breaks some use case.
- **Over-prediction**: we predicted a file that the run doesn't produce. Now the cache claims the file exists (with some checksum) but there's nothing to cache. Restore writes phantom files.

Both cases are silent data corruption unless detected. Detection requires comparing predicted vs. actual after every run, adding I/O and complexity.

### 5. Engineering cost per tool

Shape A (built-in predictors) means writing and maintaining a predictor per tool. mkdocs alone needs YAML parsing + docs/ walk + permalink logic + plugin awareness. Sphinx's `conf.py` is literal Python — we'd need a Python interpreter to evaluate it, or settle for crude text parsing. Jekyll applies Liquid permalinks. Each tool is weeks of careful work.

Shape B (generic predict command) dodges this by making it the user's problem — but then each user either writes their own predictor or depends on a community-maintained one, which shifts fragility rather than eliminating it.

### 6. Cache invalidation for the predictor itself

The predict command is itself an input to the build graph. If `list-mkdocs-outputs.sh` changes, every downstream product that depended on its outputs might need invalidation. If mkdocs's config file changes (affecting predicted paths), the predictor must rerun before graph construction. This adds a pre-graph phase that has its own caching problem.

### 7. "Declare what you produce" defeats tools that produce variably

Some tools genuinely produce different files depending on runtime content. A Sphinx build with `todo` extension adds a `todo` page only if todos exist. A Hugo site with tag aggregation produces a page per tag discovered in content. Neither can be known without running the tool (or running a tool nearly as complex as the tool itself).

For these, prediction is either incomplete or a reimplementation of the tool.

## How this relates to what we have

The [Shared Output Directory](shared-output-directory.md) design is the **fallback for tools without a predictor**. `output_dirs` + `path_owner` + tree filtering + safe cleanup is the "I don't know your outputs, so I'll respect other processors' files and cache what I find afterward" story.

Both mechanisms would coexist:

| Creator declares              | Treated as             | Caching                       | Cross-processor deps    |
|-------------------------------|------------------------|-------------------------------|-------------------------|
| `output_dirs = [...]` only    | Opaque Creator         | One tree per build           | Only via declared files |
| `predict_command = "..."` or built-in predictor | Promoted to Mass Generator | Per-file, precise | Full — all files known  |

A user can start with `output_dirs` (zero config), then add a `predict_command` later if they want tighter caching.

## Comparison with the "explicit ordering" alternative

The [processor-ordering](processor-ordering.md) doc discusses adding `after = [...]` ordering knobs. Prediction is a fundamentally different answer:

- **Explicit ordering** says "we can't model this; let the user pin the order manually."
- **Prediction** says "we can model this if we know the outputs; let's discover them."

Prediction is principled (the graph tells the truth, as it should), but expensive to do well. Explicit ordering is cheap to add but lies about the dependency graph.

Neither is strictly better. They solve different problems:
- Prediction solves **"we don't know what will be produced."**
- Ordering solves **"we know what will be produced, but there's a side effect we can't express as a file."**

## Open questions if we ever build this

1. **Shape A or Shape B first?** Probably B — lower core-code cost, lets the ecosystem handle tool specifics.
2. **How strict is the predict/actual check?** Warn? Error? Silently accept divergence? Each choice has costs.
3. **What does a bad prediction do to the cache?** Can we detect and purge polluted entries, or do we require a clean?
4. **Should predict run on every build, or be cached like a regular input?** If cached, what invalidates it?
5. **Does the predicted output list flow into `processors files`, `--dry-run`, and `graph`?** Users will expect yes; that's engineering work.

## Recommendation

**Don't build this yet.** The shared-directory design handles the 80% case without tool-specific code. Prediction is a big feature with many edges — worth doing only if:

- Multiple concrete use cases demonstrate that `output_dirs` + shared-directory semantics is insufficient.
- At least one target tool (probably mkdocs or Sphinx) has a stable, scriptable way to list its outputs — so Shape B has a working reference case.
- The predict/actual validation story is designed before the feature ships.

If and when we build it: start with Shape B (`predict_command` field), implement it for one tool as a proof of concept, and iterate.

## See also

- [Shared Output Directory](shared-output-directory.md) — how we handle opaque Creators today
- [Processor Ordering](processor-ordering.md) — the sibling discussion about explicit ordering
- [Cross-Processor Dependencies](cross-processor-dependencies.md) — why per-file outputs enable proper dependency graphs
