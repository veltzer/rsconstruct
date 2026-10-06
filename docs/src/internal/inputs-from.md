# Explicit Processor Inputs (`inputs_from`)

**Status: proposed (2026-10-06), not implemented.**

This chapter proposes replacing the fixed-point discovery loop with an
explicit statement of which processors consume which processors' outputs.
It grew out of finding R8 in
[Architecture Observations](architecture-observations.md) ("discovery re-runs
every processor on every pass"): instead of making the loop cheaper, remove
the reason it exists.

## The problem with discovery by suffix

How a generated file reaches the processor that consumes it today
(see [Cross-Processor Dependencies](cross-processor-dependencies.md)):

1. Every processor discovers products by scanning the `FileIndex` with its
   scan fields (`src_dirs`, `src_extensions`, `src_files`, globs).
2. The outputs declared in that pass are added to the index as *virtual
   files*.
3. Every processor runs discovery again, in case one of the new paths
   happens to match its scan fields.
4. Repeat until a pass adds nothing, up to `max_discovery_passes`.

What is wrong with it:

- **Routing is a guess.** Whether `gen/x.md` reaches a processor depends on
  whether its path happens to match that processor's `src_dirs` and
  extensions. Nobody stated that the processor consumes another processor's
  outputs, and nothing checks that it does.
- **Every processor runs on every pass**, whether or not anything new can
  concern it (R8).
- **It needs machinery that exists only for re-running:** virtual files,
  superset re-declaration in `BuildGraph::add_product` (`try_update_inputs`,
  the `checker_dedup` index), the convergence check and its limit.
- **Producers that don't know their outputs are invisible.** A creator
  declares only an output directory, so nothing it writes ever becomes a
  virtual file. A processor whose `src_dirs` points into that directory
  finds nothing on a clean build and the build stays green.

## Proposal

A processor names the processors whose outputs it consumes:

```toml
[processor.generator.tera]
src_dirs = ["tera.templates"]

[processor.checker.shellcheck]
src_dirs    = ["scripts"]                       # hand-written scripts on disk
inputs_from = ["processor.generator.tera"]      # plus tera's outputs
```

### Semantics

- **What B is offered.** The files on disk, plus the declared outputs of
  every processor named in `inputs_from`. Nothing else: outputs of
  processors B does not name are not offered, however well their paths
  match.
- **What B takes.** B's scan fields filter what it is offered, exactly as
  they filter the disk today. The filter is not routing — the census below
  shows consumers routinely take a subset of a producer's outputs (one
  subdirectory, one extension), so a filter is needed either way, and
  reusing the scan fields keeps a single vocabulary.
- **Order.** `inputs_from` makes a graph of processors. Discovery runs once,
  over that graph in dependency order: when B discovers, every processor it
  names has already declared its outputs. A cycle is a config error found
  at load time, naming the processors on it.
- **Rebuilds.** `inputs_from` is data flow, not an ordering hint. B's
  products have A's outputs as inputs, so product edges, checksums and
  rebuilds follow from the normal mechanisms. It is not the `after` knob
  that [Processor Ordering](processor-ordering.md) argues against: it
  carries rebuild semantics, and it is the *only* place the relationship is
  stated (dilemma 2 there, "two sources of truth", does not arise).

### Naming versus searching

A literal path is already exact: an explicit processor's
`inputs = ["out/tera/books/openbook.ly"]`, a `src_files` entry, or a
non-glob `dep_inputs` entry names one file, and the product edge to its
producer comes from path matching as today. Such references need no
`inputs_from`.

Anything that *searches* — a `src_dirs` entry, an `input_globs` or
`dep_inputs` glob — must name its producer. The rule:

> Searching inside another processor's outputs requires naming that
> processor in `inputs_from`. Naming a single file does not.

### Strictness

Each of these is a config error, reported with the processor and path
involved and the fix:

1. A search field (`src_dirs`, `input_globs`, glob `dep_inputs`) reaches
   into another processor's declared outputs or output directory, and that
   processor is not in `inputs_from`. The message names the producer to add.
2. `inputs_from` names an unknown processor.
3. `inputs_from` names a processor that declared outputs, none of which pass
   B's filter — a connection that delivers nothing is a typo or a stale
   stanza. A producer with *no* outputs (disabled, or nothing to do in this
   repo) is not an error: one shared `rsconstruct.toml` serves repos of
   different layouts.
4. A per-file consumer names a producer that cannot predict its outputs,
   without opting into discovery during the build (see tier 3 below).

## Producers that don't know their outputs

A creator (sphinx, mdbook, jekyll, cargo, pip, npm, generic creators)
declares an output directory and learns what is in it only by running.
Handing over outputs requires knowing them, so these get three tiers.

### Tier 1 — make it predictable

A [mass generator](output-prediction.md) runs `predict_command` before the
build and declares every planned file, then checks after running that the
tool wrote what it planned. To `inputs_from` it is an ordinary producer
with known outputs and per-file handoff. Any tool that can say in advance
what it will produce should be wired this way.

### Tier 2 — hand over the directory as one input

Many consumers of a creator's output want the whole tree: a link checker
over `_site`, an HTML validator run once, an archive, a deploy. Naming a
creator in `inputs_from` gives B **one product** whose input is the
creator's output tree.

- **Order** is known before anything runs; the edge comes from the config.
- **The input checksum** is taken at dispatch, after the creator ran (the
  executor decides every product at dispatch — R1). It is computed from the
  creator's tree descriptor, which the object store records on every build
  or restore (`ObjectStore::record_last_tree`). That list is exact: it
  excludes files other processors contribute to a shared directory, which a
  directory walk would not (see [Shared Output Directory](shared-output-directory.md)).
- This needs a new kind of product input — a producer's tree rather than a
  path — in the graph, the checksum code and `product show`.

### Tier 3 — one product per file, discovered after the creator runs

Per-file checking of a creator's output (one bad page fails one product, an
unchanged page is skipped) needs products that can only be created once the
creator has run. The executor's `GraphRefresh` hook, added for mid-build
re-analysis (R5), is where it would happen: after the creator's level, B
discovers products from the tree the creator actually wrote, they join the
graph, and the remaining levels are re-planned.

Costs:

- `status`, `--dry-run` and the predicted counts cannot see these products
  before the creator runs. They could show the last recorded tree, marked
  as provisional.
- It is the only place products are created during execution. Today the
  product set is fixed before anything runs (observation #6).
- Clean and stale-output removal must know these products belong to B; the
  recorded tree is again the source.

Tier 3 is opt-in on the consumer (e.g. `inputs_from_mode = "per_file"`) and
is built only when a real config needs it — the census found none.

## Analyzers are unchanged

`inputs_from` decides which *products* exist. Dependencies found inside
files — an `#include`, an image reference — are a different relation and
stay with the analyzers: they resolve references against every declared
output and re-analyze sources regenerated mid-build (R5). A markdown page
referencing a generated image does not need its processor connected to the
image's producer; the reference itself is the data flow.

## What goes away

- The fixed-point loop, `max_discovery_passes` and its convergence error.
- Virtual files (`FileIndex::add_virtual_files`). Each processor gets a view
  of the index: the files on disk plus the outputs of the processors it
  names. Analysis keeps one view with all declared outputs.
- Re-declaration handling in `BuildGraph::add_product`: superset input
  updates (`try_update_inputs`) and the `checker_dedup` index exist because
  processors rediscover the same products on later passes. With one pass a
  re-declaration is an error.
- R8, which this replaces.
- A minor inconsistency: `-p B` could pull in B's producers, as `--target`
  already does, since they are known from the config.

## Migration

Census of the 223 `rsconstruct.toml` files under `~/git`, matching search
and path fields against `out/` and against other processors' declared
`output_dir`/`output_dirs`. The match is textual (default output
directories were not resolved), so the counts are approximate.

| Repo | Sites | Kind |
|------|-------|------|
| book-openbook | 25 | `src_dirs` into tera / generic outputs; `input_globs` over `out/derived`; literal `inputs` of generated `.ly`/`.pdf` |
| teaching-syllabi | 6 | five processors with `src_dirs` on one generic generator's output; one glob over `_site/tracks` |
| rsslide | 2 | `src_dirs = ["out/import"]` |
| teaching-slides | 2 | globs over `_site/pdfunite`, `_site/marp` |
| veltzer.github.io | 2 | `src_dirs` on isass output; literal input from a mass generator |
| teaching-animations | 1 | glob over `_site/animations` |

About 38 sites in 6 repos. The literal `inputs` (roughly ten, mostly in
book-openbook) stay as they are. The rest — `src_dirs` and globs reaching
into generated directories — each gain an `inputs_from` line. All producers
found are generators or mass generators with known outputs: **no config
consumes an unpredictable creator's output today**, so tiers 2 and 3 have
no current users.

There are no external users, so there is no compatibility period: strictness
rule 1 turns every unmigrated site into a load error naming the producer to
add, and the fleet is migrated in the same change.

## Implementation plan

1. Add `inputs_from` to the standard scan config (one `FieldSpec`), resolve
   names through `registries::parse_name`, build the processor graph and
   reject cycles at config load.
2. Discovery in processor-graph order, each processor scanning its own
   index view. Keep the fixed-point loop for processors without
   `inputs_from` so both mechanisms coexist for one release.
3. Strictness rules 1–3 as errors; migrate the fleet repos listed above.
4. Delete the loop, virtual files, `max_discovery_passes` and the
   re-declaration machinery. Update [Cross-Processor
   Dependencies](cross-processor-dependencies.md) to describe the new
   mechanism.
5. Tier 2 (tree inputs from creators).
6. Tier 3 only when a config needs it.

## Decisions

- **Instances, not types.** `inputs_from` takes instance names — the same
  name as the config header and the `-p` argument
  (`processor.generator.generic.derive_metadata`;
  `processor.generator.tera` for a processor declared without an instance
  name). A type name that has named instances is an error listing them:
  which instance feeds which is exactly what the declaration is for, and a
  type name would hide it.
- **Variants: all of them.** A producer with variants (compiler profiles)
  offers the outputs of every variant. No new syntax: variants already
  write to distinct paths (two variants declaring one path is an output
  conflict), so a consumer that wants one variant filters by path with its
  existing `src_dirs`. A dedicated selector can be added if a config ever
  needs one that a path filter cannot express.
- **Lua plugins are out of scope.** They are unmaintained; this design
  does not cover them, and nothing in it is promised to work for them.

## Relation to earlier designs

- [Cross-Processor Dependencies](cross-processor-dependencies.md) listed
  explicit wiring as Approach D and set it aside as "more configuration
  burden" layered on top of scanning. Here the declaration *replaces* the
  search across all outputs instead of duplicating it, which is what
  changes the trade-off.
- [Processor Ordering](processor-ordering.md) rejects ordering-only knobs;
  `inputs_from` is a data dependency (see Semantics).
- [Output Prediction](output-prediction.md) is tier 1.
- [Shared Output Directory](shared-output-directory.md)'s ownership rule is
  what makes a tier 2 tree input exact.
