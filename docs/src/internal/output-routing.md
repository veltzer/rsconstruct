# Routing Generated Files Between Processors

**Status: proposed (2026-10-06), not implemented.**

This chapter proposes replacing the fixed-point discovery loop with
*routing*: every file a processor declares as an output is handed to exactly
the processors that accept it, and only those processors do any further
discovery. Connections between processors stay automatic — inferred from
the scan settings users already write — and an optional `inputs_from` field
covers the cases inference cannot. It grew out of finding R8 in
[Architecture Observations](architecture-observations.md) ("discovery re-runs
every processor on every pass").

## Today: the fixed-point loop

How a generated file reaches the processor that consumes it today
(see [Cross-Processor Dependencies](cross-processor-dependencies.md)):

1. Every processor discovers products by scanning the `FileIndex` with its
   scan settings (`src_dirs`, `src_extensions`, `src_files`, globs).
2. The outputs declared in that pass are added to the index as *virtual
   files*.
3. **Every** processor runs discovery again over the **whole** index, in
   case one of the new paths matches its settings.
4. Repeat until a pass adds nothing, up to `max_discovery_passes`.

What is good about it, and must be kept: **connections are automatic.** If
processor A generates `gen/doc.md` and processor B's settings accept `.md`
files under `gen/`, B consumes it. Nobody has to say so.

What is wrong with it:

- **Every processor re-runs on every pass**, over the whole index, whether
  or not anything new can concern it (R8).
- **It needs machinery that exists only for re-running:** virtual files,
  re-declaration handling in `BuildGraph::add_product` (the superset input
  update in `try_update_inputs`, the `checker_dedup` index), the
  convergence check and its limit.
- **The resulting wiring is invisible.** Which processor feeds which is a
  side effect of path matching; nothing shows it.
- **Producers that don't know their outputs are invisible.** A creator
  declares only an output directory, so nothing it writes ever becomes a
  virtual file. A processor whose `src_dirs` points into that directory
  finds nothing on a clean build, and the build stays green.

## Proposal: route each new file to the processors that accept it

The rule for whether B consumes a file is unchanged: B's scan settings
accept its path. What changes is that the rule is applied **per file, at
the moment the file is declared**, instead of by re-scanning everything:

1. **Pass 0.** Every processor discovers products from the files on disk,
   as today.
2. **Route.** Each output declared by a new product is checked against
   every processor's scan settings. Each processor that accepts the path is
   handed that one file.
3. **Discover the file.** A processor handed a file creates the products
   for that file — and nothing else; it does not rescan.
4. **Repeat** with the outputs those products declare, until no new file
   is declared.

Routing is a path match against each processor's settings — cheap compared
with a discovery run, and done once per new file instead of once per
processor per pass. A processor that accepts nothing new never runs again.

### What it keeps and what it removes

- **Kept:** automatic connections, with exactly today's matching rule, so
  every existing config behaves the same.
- **Removed:** re-running every processor, scanning the whole index per
  pass, virtual files as a mechanism for discovery, and the re-declaration
  handling for per-file processors (a per-file processor is handed each file
  once).
- **Added:** the wiring is a computed fact. Discovery records which
  processor's outputs went to which processor, so it can be shown
  (`rsconstruct graph processors`, or similar) and checked by `inputs_from`
  assertions (below).

The convergence limit (`max_discovery_passes`) can stay as a guard against a
runaway chain, but is no longer how discovery ends in the normal case: it
ends when the routing queue is empty.

## Processors that need all their inputs at once

Most processors are **per-file**: one product per input file (generators,
most checkers). Handing them one file at a time is natural.

Some build **one product from a whole set of files**: `tags` (one index over
all sources), `duplicate_files` (a whole-set comparison), `pdfunite` and
`ipdfunite` (many PDFs into one), explicit processors (inputs given as
globs), and creators whose product depends on a glob of files. These are
**aggregate** processors. Handing them one file at a time produces a product
over a partial set, which today's code repairs by growing the product's
inputs on later passes.

Under routing, an aggregate processor waits instead of being repaired:

- Files routed to it are **collected**, not discovered one by one.
- It discovers **once**, with its full set, when no processor that could
  still route a file to it has work left — every per-file chain that could
  feed it has drained.
- Whether a processor *could* still feed it is known: the set of producers
  whose declared outputs it accepted so far, plus the processors upstream
  of those in the routing graph.

So discovery alternates: drain the per-file routing queue, then run the
aggregate processors whose producers have all settled, route their outputs,
drain again. An aggregate processor's outputs that route (directly or
through others) back to an aggregate processor that feeds it is a cycle,
reported as a config error naming the processors on it.

Whether a processor is per-file or aggregate is a property of the processor,
declared in its plugin entry next to the rest of its schema — one file per
processor, as the coding rules require. A processor that does not declare it
is treated as aggregate: always correct (it waits for its full set), at
worst later than necessary.

This waiting is the part of the design that needs the most care and the
most tests: chains that alternate per-file and aggregate stages
(templates → pages → `tags` → generated index page → checker), and the
cycle report.

## `inputs_from`: optional, for what inference cannot do

```toml
[processor.checker.linkcheck]
inputs_from = ["processor.creator.mdbook"]
```

`inputs_from` names processors by instance name — the name in the config
header and the `-p` argument (`processor.generator.generic.derive_metadata`;
`processor.generator.tera` for a processor declared without an instance
name). A type name with named instances is an error listing them. Most
configs never need it. It exists for three cases:

1. **Connect what cannot be inferred.** A creator declares an output
   directory and learns what is in it only by running, so its files cannot
   be routed in advance. Naming it in `inputs_from` is the only way to
   consume it; see the next section.
2. **Restrict.** With `inputs_from` set, B accepts generated files only from
   the processors it names, instead of from every processor whose outputs
   happen to match its settings. For two producers writing similar paths
   when only one should feed B.
3. **Assert.** When the connection would be inferred anyway, naming it
   turns "B consumes A's files" into a checked fact: the build fails if
   discovery routed nothing from A to B — a moved directory or a changed
   extension is caught instead of silently checking less. A producer with
   *no* outputs (disabled, or nothing to do in this repo) does not fail the
   assertion: one shared `rsconstruct.toml` serves repos of different
   layouts.

Variants: a producer with variants (compiler profiles) offers the outputs of
every variant. Variants write to distinct paths, so a consumer that wants
one variant filters with its existing `src_dirs`.

## Producers that don't know their outputs

A creator (sphinx, mdbook, jekyll, cargo, pip, npm, generic creators)
declares an output directory and learns what is in it only by running.
Three tiers:

### Tier 1 — make it predictable

A [mass generator](output-prediction.md) runs `predict_command` before the
build and declares every planned file, then checks after running that the
tool wrote what it planned. Its outputs route like any generator's. Any tool
that can say in advance what it will produce should be wired this way.

### Tier 2 — hand over the directory as one input

Many consumers of a creator's output want the whole tree: a link checker
over `_site`, an HTML validator run once, an archive, a deploy. Naming a
creator in `inputs_from` gives B **one product** whose input is the
creator's output tree.

- **Order** is known before anything runs: the connection is declared.
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
re-analysis (R5), is where it would happen: after the creator's level, its
actual files are routed like declared outputs, products join the graph, and
the remaining levels are re-planned.

Costs: `status`, `--dry-run` and the predicted counts cannot see these
products before the creator runs (they could show the last recorded tree,
marked provisional); it is the only place products are created during
execution; clean and stale-output removal must learn these products from
the recorded tree.

Tier 3 is opt-in on the consumer (e.g. `inputs_from_mode = "per_file"`) and
is built only when a real config needs it.

### Reporting the gap

Today a processor whose `src_dirs` points into a creator's output directory
silently finds nothing on a clean build. Under routing that becomes a
config error: a search setting reaches into a directory owned by a creator
that is not named in `inputs_from`. The message names the creator and the
tiers above. This is the one place routing asks the user for anything, and
only where inference is impossible.

## Analyzers are unchanged

Routing decides which *products* exist. Dependencies found inside files —
an `#include`, an image reference — stay with the analyzers: they resolve
references against every declared output and re-analyze sources regenerated
mid-build (R5).

## Migration

Census of the 223 `rsconstruct.toml` files under `~/git` (textual match of
search and path settings against `out/` and other processors' declared
output directories; approximate):

| Repo | Sites | Kind |
|------|-------|------|
| book-openbook | 25 | `src_dirs` into tera / generic outputs; `input_globs` over `out/derived`; literal `inputs` of generated `.ly`/`.pdf` |
| teaching-syllabi | 6 | five processors with `src_dirs` on one generic generator's output; one glob over `_site/tracks` |
| rsslide | 2 | `src_dirs = ["out/import"]` |
| teaching-slides | 2 | globs over `_site/pdfunite`, `_site/marp` |
| veltzer.github.io | 2 | `src_dirs` on isass output; literal input from a mass generator |
| teaching-animations | 1 | glob over `_site/animations` |

Every producer found is a generator or mass generator with known outputs,
so every one of these connections is inferred by routing exactly as it is
by the loop today. **No config changes are needed**, and no config consumes
a creator's output directory, so the new error above fires nowhere today.

These repos are the regression suite for the change: each must discover the
same products before and after (`rsconstruct graph` output compared), with
book-openbook's mixed per-file and glob-based chains the main exercise for
the aggregate waiting.

## Implementation plan

1. A per-file/aggregate property in the processor plugin entry; default
   aggregate.
2. A routing function: path → processors whose scan settings accept it.
   Reuses the existing scan-setting matching so the rule cannot drift.
3. Discovery as a routing queue: pass 0, per-file discovery of routed files,
   aggregate processors once their producers have settled. Keep the
   fixed-point loop behind a switch for one release and compare the
   resulting graphs on the fleet.
4. Record the routing graph; show it in `graph`; cycle errors.
5. Delete the loop, virtual files as a discovery mechanism, and the
   re-declaration handling in `add_product`.
6. `inputs_from` (restrict and assert), and the creator-output config error.
7. Tier 2 (tree inputs from creators). Tier 3 only when a config needs it.

## Decisions

- **Inference is the default; `inputs_from` is optional.** Users connect
  nothing by hand unless a producer cannot predict its outputs.
- **Instances, not types,** in `inputs_from`.
- **Variants: all of them**, filtered by path when needed.
- **Lua plugins are out of scope.** They are unmaintained; this design
  does not cover them.

## Relation to earlier designs

- [Cross-Processor Dependencies](cross-processor-dependencies.md): the
  fixed-point loop (its Approach C) is kept in behavior — same automatic
  connections — and replaced in mechanism. Its Approach D, explicit wiring
  in config, survives only as the optional `inputs_from`.
- [Processor Ordering](processor-ordering.md) rejects ordering-only knobs;
  `inputs_from` is a data dependency with rebuild semantics.
- [Output Prediction](output-prediction.md) is tier 1.
- [Shared Output Directory](shared-output-directory.md)'s ownership rule is
  what makes a tier 2 tree input exact.
