# Feature design: expose build-graph metadata to Tera templates

## Status

Draft — awaiting review.

## Origin

`problems.txt`:

> There is knowledge that we know when we run the tera processor (for
> instance, how many files are under the charge of each processor) - we
> can make that knowledge available to the processor so we wouldn't have
> to do things like counting files.

## What's there today

The Tera processor (`src/processors/generators/tera.rs`) renders
templates with **an empty `TeraContext`**. Templates have access to
only the four custom Tera functions registered by rsconstruct:

| Function          | Purpose                                                  |
| ----------------- | -------------------------------------------------------- |
| `workflow_names()`| Names of GitHub Actions workflows in `.github/workflows` |
| `shell_output()`  | Run a shell command, return stdout                       |
| `glob()`          | List files matching a glob pattern                       |
| `grep_count()`    | Count files matching a content pattern                   |

So when a template wants to say "this project has 1234 markdown files
under the syllabi processor", it has to do:

```
{{ glob(pattern="syllabi/**/*.md") | length }}
```

That works but it duplicates work the build graph already did during
discovery, and it doesn't match the *processor's* view of what files
it owns (which involves `src_dirs`, `src_extensions`,
`src_exclude_dirs`, `src_exclude_files`, and the analyzer's own logic).
A better question is "ask the markdownlint processor how many products
it has".

## The problem in one sentence

The build graph knows everything about every processor's products,
but the tera processor — which runs *during* the build — has no way
to query it.

## Why this is harder than it looks

The natural place to hand the graph to a processor is via
`Processor::execute(&self, ctx, product) -> Result<()>`. But:

- `ctx` is a `BuildContext` that intentionally does *not* own the
  graph. The graph is built by `Builder`, lives in `Executor`, and is
  passed by reference along the executor's call chain.
- Adding the graph (or a reference to it) to `BuildContext` makes
  every processor able to read it, which sounds nice but is a real
  architectural change: the graph is huge and held under several
  invariants the executor maintains. Random read access from a
  processor's worker thread isn't currently safe.
- The graph is mid-mutation while processors are executing — a
  generator processor's `execute` may be the very call that produces
  outputs that another processor will scan in a later pass. "Read the
  graph" needs to be defined as "the snapshot at the moment your
  execute started", not "live state".

## Three options

### Option A — TeraContext gets project metadata at render time

Build a small "metadata snapshot" at render time and shove it into
`TeraContext` as variables:

```jinja
{{ processors.markdownlint.product_count }}
{{ processors.markdownlint.source_count }}
{{ processors.ruff.products[0].file }}
{{ project.total_products }}
{{ project.processor_names }}
```

Implementation:

- The tera processor's `execute` is called from the executor, which
  has the graph. We need to thread a graph reference (or a snapshot)
  through to `execute`.
- Build a serde-serializable `ProjectSnapshot { processors: Map<name,
  ProcessorSnapshot>, total_products: usize, ... }` from the graph.
- Insert it into TeraContext as `processors` and `project`.

This is the most direct answer to the user's request and the tightest
contract: templates ask for what they want, get a value.

The cost is plumbing: `Processor::execute`'s signature doesn't take the
graph today. We can either:

- **A1**: Add `graph: &BuildGraph` to `execute` (every processor's
  signature changes). Most processors don't care.
- **A2**: Add `graph: &BuildGraph` only to a separate trait
  `MetadataAware` or similar that tera implements, and have the
  executor check for it.
- **A3**: Stash the snapshot in `BuildContext` before calling
  `execute`, since BuildContext is already passed everywhere. The
  executor takes a one-time snapshot at the start of each level and
  installs it on `BuildContext`. Tera reads it.

A3 is the smallest-blast-radius change and matches how
`set_declared_tools` already works (a thread-local lookup with a
documented lifetime). Recommend A3.

### Option B — new Tera functions backed by graph queries

Instead of injecting variables into the context, add Tera functions
that the template can call:

```jinja
{{ processor_count(name="markdownlint") }}
{{ processor_files(name="markdownlint") | length }}
{{ processor_outputs(name="ruff") }}
```

Same data, different shape. The function approach has two upsides:

- Lazy: templates only pay for what they ask for. With variables in
  context, we precompute everything once per render.
- Filterable: `processor_files(name="x", ext="py")` is natural; with
  context variables, the user has to filter in template-land.

Implementation: same plumbing (need access to a graph snapshot at
render time), different surface.

### Option C — both

A user-friendly default snapshot in `processors.X.product_count` for
the common case, plus two or three richer functions for filtering.

## What to expose

For any of A/B/C, the question is what fields:

| Field on `ProcessorSnapshot`           | Cheap to compute? |
| -------------------------------------- | ------------------ |
| `name` (processor iname)               | yes                |
| `type_name` (`ruff`, `cc`, ...)        | yes                |
| `product_count`                        | yes                |
| `source_count` (unique input files)    | yes                |
| `output_count`                         | yes                |
| `enabled` (per project config)         | yes                |
| `is_native`                            | yes                |
| `sources: Vec<PathBuf>`                | medium (memory)    |
| `outputs: Vec<PathBuf>`                | medium (memory)    |
| `extensions: Map<ext, count>`          | medium             |

I'd ship `name`, `type_name`, `product_count`, `source_count`,
`output_count`, `enabled`, `is_native` in v1. The list-typed fields
(sources, outputs, extensions) get added on demand — most templates
that count don't need full lists.

For project-wide:

| Field on `ProjectSnapshot`                   |
| -------------------------------------------- |
| `total_products`                             |
| `processor_names: Vec<String>`               |
| `processors: Map<String, ProcessorSnapshot>` |

## Cache and consistency

The graph is mutated as products are discovered, but by the time a
processor's `execute` runs, discovery is done for that level. The
snapshot is stable for the duration of one render call. We should
*not* re-snapshot during render; one snapshot per `execute` call is
correct.

The cache key for the tera product needs to include the snapshot
content, otherwise a template that says "we have 50 files" stays
cached even after the project grows to 100. Easiest: include a
**hash of the snapshot's serialized form** as a config_hash piece on
each tera product. The existing `extend_config_hash` mechanism
(`Product::extend_config_hash`) is for exactly this kind of thing.

That's a real correctness concern. Without it, the feature is a
silent rebuild bug.

## Recommendation

Ship **Option A3 (variables-in-context via BuildContext snapshot) +
hash piece for cache invalidation**. Add Option B (custom Tera
functions) only if templates start needing filtered queries.

Implementation outline:

1. Add a new `pub struct ProjectSnapshot { ... }` and
   `pub struct ProcessorSnapshot { ... }` in
   `src/processors/mod.rs` (or a new `src/snapshot.rs`). Both are
   serde-serializable.
2. Add a `snapshot: Mutex<Option<Arc<ProjectSnapshot>>>` field to
   `BuildContext`. The executor populates it once per build.
3. The executor takes the snapshot at the start of each level (or
   once before starting the tera processor's level — we can be more
   targeted). The snapshot is whatever the graph looks like at that
   moment. Stored as `Arc` so it's cheap to clone into a worker.
4. Tera's `render_template` reads the snapshot from the context and
   inserts it into TeraContext as `processors` (the per-processor
   map) and `project` (the project-wide block).
5. The tera processor's `extend_config_hash` adds a hash of the
   serialized snapshot, so the cache invalidates when project shape
   changes.

Estimated ~150 lines.

## Open questions

1. Snapshot timing: after all discover passes settle (cleanest), or at
   start of the level the tera processor runs in (cheapest)? The
   former gives a stable, complete snapshot; the latter is what's
   actually known when tera runs. I'd pick after-settle — a tera
   template that says "we have 100 files" should mean the final 100,
   not "100 minus whatever discovery hadn't run yet".

2. What about non-tera processors — should they be able to read the
   snapshot too? With option A3 (it lives in BuildContext) they
   automatically can. Whether that's encouraged or considered an
   abuse is a policy choice. I'd document it as "intended for tera;
   other processors should not depend on snapshot contents."

3. Cache invalidation granularity: hash the entire snapshot, or only
   the per-processor entry the template referenced? Hashing the whole
   snapshot is simple and slightly over-conservative (the tera
   product rebuilds whenever *any* processor's product count changes,
   even ones unrelated to the template). Per-template-reference
   tracking is much harder. Start with whole-snapshot.

4. JSON shape: `processors.markdownlint.product_count` (nested) vs
   `processors_markdownlint_product_count` (flat). Tera handles
   nested fine. Nested wins.

5. Naming: variable name `processors` vs `processor_stats` vs
   `proc_info`. Tera's other globals (`workflow_names`) are
   noun-shaped; `processors` matches. Recommend `processors` and
   `project`.

## What I want from you before writing code

- Confirm Option A3 is the right shape (variables in context, not
  Tera functions). Or push me toward B/C.
- Confirm the field list (`name`, `type_name`, `product_count`,
  `source_count`, `output_count`, `enabled`, `is_native`) covers the
  use cases you have in mind. If you want full source lists too in
  v1, I'll include them.
- Confirm "snapshot taken after all discover passes settle" — the
  alternative makes the feature less useful but has a smaller code
  surface.
