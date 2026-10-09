# Routing Generated Files Between Processors

**Status: routing implemented (2026-10-06). Making whole-index processors
wait was set aside (see [Why whole-index processors do not
wait](#why-whole-index-processors-do-not-wait)); `inputs_from` and the
creator tiers are deferred until a config needs them.** See
[Implementation status](#implementation-status) at the end.

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

1. A per-file/aggregate property on each processor; default aggregate.
   **Done**, as a trait method (see below).
2. A routing function: path → processors whose scan settings accept it.
   Reuses the existing scan-setting matching so the rule cannot drift.
   **Done**, by construction (see below).
3. Discovery as a routing queue: pass 0, per-file discovery of routed files,
   aggregate processors once their producers have settled. **Partly done:**
   per-file processors are routed; aggregate processors still rediscover
   every round rather than waiting — **and will**, see below.
4. Record the routing graph; show it in `graph`; cycle errors. **Already
   there:** `rsconstruct processor graph` (text, dot or JSON) prints which
   processor feeds which, from the product edges routing creates. A cycle
   between products is a graph error at topological sort; between
   processors it is not an error (two processors may feed each other
   different files).
5. Delete the loop, virtual files as a discovery mechanism, and the
   re-declaration handling in `add_product`. **Set aside** with step 3's
   waiting, which it depends on.
6. `inputs_from` (restrict and assert), and the creator-output config error.
7. Tier 2 (tree inputs from creators). Tier 3 only when a config needs it.

## Why whole-index processors do not wait

The plan had each `WholeIndex` processor wait until no processor that could
still feed it had work left, then discover once — which would let the loop
and its re-declaration handling go. That ordering cannot be computed:

- A processor is `WholeIndex` precisely because "which files does it take"
  is not a path test on its scan settings — explicit globs, sibling
  lookups, a whole set, a plan. So whether a file *could* reach it is not
  known in advance.
- Whether one whole-index processor feeds another is only known after the
  first has discovered (its outputs come from its own discovery: tera's
  templates, a mass generator's plan).

Ordering them soundly needs every processor to predict its outputs before
discovering — Approach F in [Cross-Processor
Dependencies](cross-processor-dependencies.md), set aside there because it
duplicates each processor's output-path logic. Without it, a whole-index
processor that ran "too early" would miss files and nothing would notice.

What exists is correct and cheap: per-file processors (80 of 97) are routed
only the new files; whole-index processors rescan the index in each round
that declared new files, exactly as before, and their re-declarations are
merged by `add_product`. Discovery was never the expensive phase. This is
the endpoint unless output prediction is ever added for another reason.

## Implementation status

**What is built.** `Processor::discovery()` returns `Discovery::PerFile` or
`Discovery::WholeIndex`. In every round after the first,
`Builder::discover_products` hands a per-file processor a `FileIndex` of
only the files the previous round declared, and runs every other processor
over the whole index as before. The processor's own `discover` runs on that
smaller index, so "which new files does it accept" is decided by its own
`FileIndex::scan` — the routing rule and the scan rule cannot drift apart,
because they are the same code. `scan` judges each path on its own, which is
what makes discovering over a subset exact.

**Departures from the plan above:**

- **A trait method, not a plugin-entry field.** A field would have to be
  added to all ~100 plugin entries, and a field cannot default safely: the
  wrong value for a processor whose `discover` looks at more than one file
  would silently lose products. The trait method defaults to `WholeIndex`,
  which is correct for every processor; `PerFile` is an opt-in
  optimization.
- **Aggregate processors re-run, they do not wait (yet).** A `WholeIndex`
  processor rediscovers over the whole index in every round that declared
  new files — exactly the old loop's behavior — so the re-declaration
  handling in `add_product` stays. Making them wait for their producers
  (step 3) is what would let step 5 delete it.
- **No switch back to the old loop.** Instead of shipping both mechanisms,
  the new discovery was compared against the released 0.9.106 binary on
  copies of all 222 repos under `~/git` with an `rsconstruct.toml` (all but
  `virtual-windows`, which is VM images), with build outputs removed so
  every chain is a clean-checkout chain: `processor files --json`, sorted,
  identical in every repo — 50,930 products, all exit codes 0.

**Marked `PerFile`:** every processor whose discovery qualifies, each
audited to touch the index only through `scan`, make one product per
scanned file, and store nothing taken from the index:

- the `SimpleChecker` and `SimpleGenerator` wrappers (most checkers and
  generators);
- the checkers using the default `discover`: `ascii`, `encoding`,
  `iactionlint`, `icpplint`, `idockerfile`, `ijq`, `ijsonlint`, `isvglint`, `itaplo`,
  `itidy`, `ixmllint`, `iyamllint`, `iyq`, `json_schema`, `marp_images`;
- custom checkers: `aspell`, `clang_tidy`, `iyamlschema`,
  `license_header`, `markdownlint`, `mdl`, `script`, `terms`, `zspell`;
- custom generators: `cc_single_file`, `generic`, `ijinja2`, `jinja2`,
  `mako`, `marp`, `pdflatex`, `rust_single_file`;
- creators: `cc`, `generic`, `linux_module`, `pip`. (`cc` and
  `linux_module` read their manifest from disk, which both index views
  share; `cc` also collects compiler names, but only adds to that set.)

**Left `WholeIndex`, and why:**

| Processor | Its discovery depends on |
|---|---|
| `generator.tera` | the set of includable templates, recorded from the whole index |
| `generator.requirements`, `generator.tags`, `checker.duplicate_files` | one product over the whole set of scanned files |
| `generator.pdfunite`, `generator.ipdfunite` | files grouped by directory |
| `explicit.generic` | globs resolved against the index |
| `creator.mdbook`, `creator.jekyll`, `checker.clippy`, `checker.make`, `creator.cargo`, `creator.npm`, `creator.gem`, `creator.sphinx` | sibling files collected around an anchor |
| `mass_generator.generic`, `mass_generator.zola` | a predicted plan over the source set |

Lua plugins are out of scope and keep the default.

**Guard.** `per_file_discovery_over_a_subset_matches_the_full_index`
(in `src/processor/mod.rs`) configures every `PerFile` processor — 80 of
the 97 — to scan a fixture written to a temporary directory, and checks
that discovering over half of it gives exactly the full index's products
for that half. Every `PerFile` processor must produce products from the
fixture, so none passes vacuously. Marking mdbook (which collects sibling
files) `PerFile` makes it fail. It compares products, not state a
`discover` stores for later — the reason tera stays `WholeIndex` has to be
caught in review.

**Effect.** Discovery-only runs (`build --stop-after discover`, release
builds, best of seven) on the chain-heavy repos: teaching-syllabi 43 → 32
ms, teaching-slides 89 → 78 ms, book-openbook unchanged at ~15 ms (its
consumers are tera and explicit processors, which still rediscover).
Discovery was never the expensive phase; the gain is real but small.

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
