# Architecture Observations

Observations about rsconstruct's high-level structure — the shapes that
determine how the system behaves when you try to change or extend it. Kept
separate from `suggestions.md` (which is tactical features and bugs) because
these are about *how the code is put together*, not about what it does.

Each entry has:
- A short title naming the pattern or tension.
- What the current code does.
- What that implies for changes / extensions / users.
- **Load-bearing**: how much of the system this shape dictates. High = touching
  it ripples everywhere. Low = localized quirk.

The entries are roughly ordered by how much they shape the rest of the codebase.

---

## The central four

### 1. The graph is the universal coupling point

Every phase — discovery, analysis, classification, execution — reads and/or
mutates the `BuildGraph`. Processors receive `&mut BuildGraph` in their
`discover()` method and are trusted to add products correctly. There's no
invariant enforcement at insertion time: empty inputs are allowed, bad dep
references are allowed, duplicate outputs are caught but duplicate *inputs*
aren't. Cycles are only detected during topological sort, late.

The graph's shape also leaks into the executor: the executor knows about
`output_dirs` (creators), `variant` (multi-format generators), `config_hash`
(cache keys), and product IDs. Adding a new product category (say, a
"phantom" product that exists for scheduling but produces no outputs)
requires touching both graph and executor.

**Implication:** the graph is the lingua franca. Any architectural change
that touches the product model — adding fields, changing what counts as a
dependency, supporting alternate execution orders — ripples into every
consumer. A healthy graph layer would have *validation* (reject ill-formed
products at insertion), *opaque access* (consumers see a trait-shaped view,
not the struct), and *observer hooks* (something watching mutations so
`--graph-stats` and `graph show` don't duplicate traversal logic).

**Load-bearing:** very high.

---

### 2. Plugin registration at link time

Every processor and analyzer submits an `inventory::submit!` entry. The
registry is populated at binary link time, and enumeration is a runtime
iteration over those entries. This is elegant for modularity — adding a
processor means adding one file, no central list to update — but it has
consequences:

- **No compile-time enumeration**: you can't write a match statement over
  all processor names, so the processor-count gets rediscovered on every
  run, and static checks (e.g. "every processor has a corresponding config
  struct") have to be runtime assertions.
- **Lua plugins are second-class**: they arrive at runtime after the static
  registry is frozen. The registry API has to tolerate two populations
  (static + dynamic) in parallel, which is why `find_registry_entry` and
  `find_analyzer_plugin` have to fall through both.
- **Ordering is alphabetical everywhere**: because `inventory` doesn't
  preserve submission order, every code path that touches plugins has to
  sort by name. This is a minor tax but it's baked in everywhere.
- **Testing requires the whole binary**: you can't instantiate a stripped-down
  registry for tests; they pull the full set. Most tests don't mind, but
  ones that want a controlled plugin set have to filter rather than inject.

**Implication:** the registration model favors modularity over
introspectability. If rsconstruct ever wants a "declarative build"
representation (think Bazel's static action graph) the plugin layer will
have to expose more schema information than it does today.

**Load-bearing:** high.

---

### 3. Config defaults are scattered, not composed — PARTIALLY ADDRESSED

Three sources of defaults apply in sequence:
1. Per-processor defaults (e.g. `ruff` → `command = "ruff"`) in a giant
   match-or-registry lookup.
2. Scan defaults (src_dirs, src_extensions) via a separate mechanism
   (`ScanDefaultsData`).
3. User TOML overrides both.

The order matters, but it's encoded across `apply_processor_defaults`,
`apply_scan_defaults`, and the serde deserialization.

**Update:** config provenance tracking (`src/config/provenance.rs`) now
records where each field came from (`UserToml { line }`, `ProcessorDefault`,
`ScanDefault`, `OutputDirDefault`, `SerdeDefault`). `rsconstruct config show`
annotates every field with its source. The defaults pipeline still applies
layers across multiple functions, but the provenance map makes it possible
to answer "where did this value come from?" without tracing the code.

The remaining gap: adding a new defaults layer (env-derived, user-global)
still means inserting into the existing function chain rather than a
declarative resolver.

**Load-bearing:** medium.

---

### 4. The executor owns too much policy — RESOLVED

**Update:** a `BuildPolicy` trait has been extracted to `src/executor/policy.rs`.
`classify_products` now delegates per-product decisions to `&dyn BuildPolicy`.
`IncrementalPolicy` implements the current skip/restore/rebuild logic.
Alternate policies (dry-run, always-rebuild, time-windowed) are now a single
trait implementation away — no executor changes needed.

**Correction (2026-10-06):** the extraction was only half done. The
executor decided every product a second time, at dispatch, by calling
`ObjectStore::needs_rebuild_descriptor` directly — so the policy governed
the printed counts but not what ran, and a non-default policy would have
been ignored by execution. Now the dispatch-time decision *is* the policy's
(`prepare_level_work` calls `BuildPolicy::classify`), and
`classify_products` is documented as a prediction. See finding R1 below.

**Load-bearing:** very high, but the tension is resolved.

---

## Structural tensions

### 5. `Processor` trait assumes `StandardConfig`, but allows bypass

The `Processor` trait has a `scan_config() -> &StandardConfig` method that
every processor must implement. The default implementations of `discover()`,
`auto_detect()`, and `supports_batch()` use this config. But processors
with richer configs (e.g. `ClippyConfig`, `CcConfig`) don't *expose* those
richer fields through the trait — they store them privately and access
them internally. The outside world only sees `StandardConfig`.

**Implication:** there's no way to ask "what config does processor X
accept?" through the trait. Introspection goes through the registry
(`known_fields`, `must_fields`, `field_descriptions`) instead, which means
the processor has to register the metadata separately from implementing
the trait. The two representations can drift: someone adds a field to
`ClippyConfig` and forgets to add it to `known_fields`.

**A healthier shape** would have one source of truth per processor — the
config struct itself — with a derive macro or trait-based reflection
generating the `known_fields` list. Or go the other direction: make the
trait parameterized (`Processor<Config>`) so introspection goes through the
type system.

**Load-bearing:** medium. Doesn't break anything today but is the root
cause of several "remembered to update both places?" bugs we've fixed.

---

### 6. Analyzers are inputs-only; they can't add products

An analyzer scans the sources of existing products, and the framework
(`analyzers::Analysis`) adds what it finds as *inputs*. (Analyzers used to
walk the graph themselves through `DepAnalyzer::analyze()`; since R3/R5
they only implement `scan()`.)
It cannot:
- Create new products (the cpp analyzer can't spawn a product for a header
  it discovered).
- Remove products.
- Change processor assignments.

This is a deliberate simplification — analyzers run in a single pass after
discovery and don't need fixed-point semantics of their own. But it means
the "dependency graph" isn't really discovered by analyzers; it's refined
by them. The actual discovery of *what exists* lives entirely in
processors.

**Implication:** if a use case arises where an analyzer legitimately needs
to produce a product — e.g. "for every `.proto` import I find, ensure
there's a product for generating the .pb.cc" — the analyzer interface
doesn't support it. You'd have to turn the analyzer into a processor, or
add a "synthesize" callback. The asymmetry between processors (can add
products) and analyzers (can only add inputs) is currently invisible but
will bite eventually.

**Load-bearing:** medium. Not a bug, but a limitation that shapes what
kinds of features are easy vs. hard.

---

### 7. Processor instance ↔ typed processor mapping is one-way — PARTIALLY ADDRESSED

A `ProcessorInstance` in the config holds `(type_name, instance_name,
config_toml)`. `Builder::create_processors()` deserializes the TOML and
produces a `Box<dyn Processor>`. Afterwards, the TOML blob is discarded.

**Update:** `ProcessorInstance` now carries a `provenance: ProvenanceMap`
that records where each field came from (user TOML with line number,
processor default, scan default, etc.). This means `config show` can
annotate fields with their source without reparsing TOML, and `smart`
commands can distinguish user-set from defaulted fields.

The remaining gap: a running `Box<dyn Processor>` still can't navigate
back to its `ProcessorInstance` or the originating TOML section. The
provenance lives on the config side, not the runtime processor side.

**Load-bearing:** medium.

---

### 8. Global state in the processor runtime — RESOLVED

**Update:** all mutable process globals have been moved into `BuildContext`
(`src/build_context.rs`):
- The three processor globals (`INTERRUPTED`, `RUNTIME`, `INTERRUPT_SENDER`)
  are replaced and deleted. `run_command` takes `&BuildContext` explicitly.
- The three checksum globals (`CACHE`, `MTIME_DB`, `MTIME_ENABLED`) are
  moved into `BuildContext`. `combined_input_checksum` and `checksum_fast`
  take `&BuildContext`.

Remaining process-wide state is all immutable or correctly scoped:
- `RuntimeFlags` — immutable after startup, doesn't vary between contexts.
- `DECLARED_TOOLS` — `thread_local!`, debug-only.
- Compiled regexes — `LazyLock<Regex>`, stateless.

**Load-bearing:** resolved. Multiple `BuildContext` instances can now run
independently (daemon mode, LSP, testing).

---

## Broader patterns

### 9. Supply-driven model everywhere

The whole pipeline — discover, classify, execute — walks every product
unconditionally. There's no demand-driven path (like `make foo` which
visits only the subgraph producing `foo`). The `--target <glob>` flag
filters *after* discovery; it doesn't trim the work that discovery itself
does.

This is a deliberate design — rsconstruct's typical workload is "build
everything incrementally," and supply-driven matches that well. But it
means a user asking "just build X" still pays the cost of discovering all
5000 other products.

**Implication:** for projects at a certain scale, or for tooling that
wants to quickly answer "which products would I run for this file?" (IDE
integration, pre-commit hooks), the supply-driven model becomes a
bottleneck. A demand-driven shortcut would require either pre-built
reverse indexes (input path → product) persisted between runs, or an
analytical model of each processor's output paths (hard — processor output
is computed procedurally).

**Load-bearing:** very high. Changing this means a fundamentally different
build-system shape.

---

### 10. "Run on every build" is the default stance

Every configured processor discovers and classifies on every invocation.
There's no concept of "processor X is slow, only run when asked." The
`-p`/`-x` mechanism works per-invocation but not as a declarative
property. See `suggestions.md` for the proposed `build_by_default = false`
pattern — that's a tactical fix. The architectural observation is that
rsconstruct's model biases hard toward "all processors together,"
whereas the user mental model often has lifecycle phases (lint vs.
package vs. deploy).

**Implication:** adding a "goals" layer (cargo-style subcommands, or
npm-style named scripts) is a natural extension direction. It would
introduce a new concept — a *goal* is a named selection of processors —
and likely requires CLI reorganization. Bigger than it sounds.

**Load-bearing:** medium. Shapes the CLI surface and user mental model.

---

### 11. Object store as a multi-responsibility module — RESOLVED

**Update:** `ObjectStore` has been decomposed into focused submodules:
- `blobs.rs` — content-addressed blob storage (store, read, restore, checksum)
- `descriptors.rs` — cache descriptor CRUD (store_marker, store_blob, store_tree)
- `restore.rs` — cache query and restoration (restore_from_descriptor,
  needs_rebuild, can_restore, explain)
- `management.rs` — cache management (size, trim, remove_stale, list, stats)
- `operations.rs` — remote cache push/fetch
- `config_diff.rs` — processor config change tracking

`mod.rs` went from ~664 to ~223 lines (struct definition, types, constructor).
Each concern is now a focused 100–150 line file.

**Load-bearing:** very high, but the monolith is resolved.

---

## What's absent that one might expect

### 12. No abstraction for "tool invocation"

Every processor that shells out to a subprocess rolls its own `Command`
building: env vars, arg construction, timeout, output capture, error
classification. Shared helpers (`run_command`, `check_command_output`)
exist but are minimal. Processor implementations still have to know about:
- How to pass files (positional args vs. `--file=X` vs. stdin vs.
  response file when argv is too long).
- How to interpret exit codes (some tools return 1 for "found issues",
  some return 0 and print to stderr, some return 2 for config errors).
- How to parse output for structured errors.

**Implication:** processor implementations have roughly 30-80 lines of
boilerplate each, and they're inconsistent. A `ToolInvocation` abstraction
with pluggable arg-passing strategies would shrink most processors to a
few lines of declaration. This also makes adding a new processor harder
than it needs to be.

**Load-bearing:** medium.

---

### 13. No pluggable reporting / event stream

Today reporting is hardcoded: `println!` during execution, colored summary
at the end, `--json` mode emits structured events, `--trace` emits Chrome
tracing format. Each reporting path is a separate code path threading
through the executor.

**Implication:** adding a new output format (JUnit XML for CI, GitHub
Actions annotations, custom Slack webhook) means threading another code
path through the executor. A proper event-bus model — executor emits
events, subscribers render them — would make this a two-file change
(subscribe + format).

**Load-bearing:** medium.

---

### 14. No formal dry-run execution

There's `--stop-after classify`, which stops after classification, and
there's `dry_run()` (different from `--dry-run` which is a flag on build),
and there's `--explain` which annotates per-product decisions. Three
partially-overlapping mechanisms. The user-facing story is "to see what
would happen, use X or Y or Z depending on what you want."

**Implication:** these evolved separately. A unified "simulation mode"
that fully runs the classify pipeline and outputs what would happen —
including what cache entries would be produced — would subsume the three.
Likely a small refactor, but requires aligning on the output shape.

**Load-bearing:** low-medium.

---

## Review of 2026-10-06: dependency management and execution stages

A read-through of the build pipeline end to end (driver, graph, discovery,
analyzers, deps cache, executor, checksums, object store). R1–R5 were
correctness defects and are fixed, as are R9 and R10; R6–R8 and the minor
items are open.

### R1. Two decisions per product, only one of them the policy's — RESOLVED

`classify_products` decided skip/restore/build through `BuildPolicy`; the
executor then decided again at dispatch with its own call to
`needs_rebuild_descriptor`, using the classification only to size the
progress bar. The `execute()` doc claimed classify-time checksums were
reused; the code recomputed them. `dep_changed` was effectively ignored at
execution (a checker predicted BUILD skipped if its marker matched), so
the "N to build" counts were wrong.

**Fix:** one decision per product, made at dispatch (all dependencies have
run, inputs are final) by the policy. `dep_changed` is gone from the
policy: with a content-addressed key the checksum *is* the dependency
signal. `classify_products` is now documented as a pessimistic prediction
that only sizes the bar, prints the counts and picks what to unlink.

### R2. The cache key was taken after the tool ran — RESOLVED

`handle_success` hashed the inputs after execution and cached the outputs
under that key, to cope with products whose `dep_inputs` glob matched their
own output. An input edited while its tool ran was cached as if the old
output belonged to the new content: the next build skipped, stale.

**Fix:** a product's own outputs are never its inputs
(`graph::without_own_outputs`). Outputs are cached under the dispatch-time
checksum; when the post-run checksum differs, the outputs are not cached
and a warning says so, so the next build reruns the product. Limitation:
the in-session checksum caches assume content is stable within a build —
input lists of 32+ files are memoized, and with `--no-mtime-cache` every
file is — so a mid-run edit is only detected through the mtime check.

### R3. Analyzers wrote inputs behind the graph's indexes — RESOLVED

Analyzers mutated `product.inputs` through `get_product_mut`, bypassing
`input_to_products`, so `products_consuming` (and `graph` lookups built on
it) never saw analyzer-found dependencies. **Fix:** `get_product_mut` is
gone; inputs grow only through `BuildGraph::add_inputs`, which keeps the
index. `resolve_dependencies` recomputes edges instead of appending, so it
can run again mid-build.

### R4. The deps cache ignored configuration and missing files — RESOLVED

Entries were keyed by `(analyzer, path)` alone and validated only against
file contents. Changing `include_paths` (or pkg-config output) kept the old
lists; a new `src/x.h` shadowing `include/x.h` went unnoticed. **Fix:**
entries carry the analyzer's config fingerprint and the paths resolution
probed and found missing (`ScanResult::absent`). See
`dependency-caching.md`.

### R5. Generated sources were never analyzed in the build that wrote them — RESOLVED

A source that did not exist at graph time was skipped (clean checkout), and
one that existed was analyzed in its stale state. Its consumer could run
before a generated header it included (no edge), and its cache key differed
between a clean and an incremental build — so a CI-filled remote cache
never hit locally, and the build after a clean build rebuilt.

**Fix:** analyzers resolve against the post-discovery file index, which
includes declared outputs, so references to not-yet-generated files become
edges. `analyzers::Analysis` outlives graph construction: after each level
the executor reports what built or restored (`executor::GraphRefresh`),
and every product whose analysis read a rewritten file is reset to its
declared inputs and re-analyzed in graph-time order. The executor then
re-links and re-plans the remaining levels. The analyzer trait shrank to
`scan()`; iteration, caching and graph updates live in the framework.

### R6. Levels are barriers (open)

`execute_parallel` runs level by level; one slow product holds up the whole
next level. A ready-queue scheduler would dispatch each product the moment
its dependencies finish — and is where R5's re-planning would naturally
live.

### R7. Work is split statically; `-j` is not a global cap (open)

Non-batch items are cut into `parallel` fixed chunks up front (no load
balancing, and a chunk blocked on a `max_jobs` semaphore stalls the rest of
it). Each batch group adds a thread on top of those, and a batching
processor runs its chunks serially.

### R8. Discovery re-runs every processor on every pass (open)

The fixed-point loop rediscovers all processors each pass, not just those
whose extensions match the newly added virtual files. Proposed fix:
[Explicit Processor Inputs](inputs-from.md), which removes the loop.

### R9. The deps cache committed once per scanned file — RESOLVED

Each `DepsCache::set` was its own durable redb transaction — one fsync per
file on a cold scan. **Fix:** `set` stages and `DepsCache::flush` writes the
batch in one transaction at the end of each analysis pass. A cold scan of
2,000 sources: 2,031 fsyncs before, 32 after.

### R10. The mtime cache trusted mtime alone — RESOLVED

`MtimeEntry` held `(mtime, checksum)`. Tools that preserve mtimes (`tar x`,
`cp -p`, `rsync -a`) changed content without changing it, and the build
skipped with stale outputs. **Fix:** entries hold a `platform::FileStamp`
(mtime, ctime, size, inode); ctime cannot be set from user space. The
"changed too recently to store" check uses the later of mtime and ctime.

### Minor (open)

- pkg-config and `include_path_commands` failures print a warning and
  continue, silently dropping dependencies — against "strict by default".
- `build_graph_filtered` is a hand-copied fourth graph-building path that
  skips tool-version hashing.
- `-p B` does not pull in the producers of B's inputs; `--target` does.
- `@<name>` aliases: `@ruff` works only because ruff is also a tool name;
  the five type aliases are copy-pasted branches.
- Five `add_product*` constructors; `src/config/mod.rs` is 3,100+ lines.
- Products predicted to build only because a dependency changes still have
  their outputs unlinked up front, so an unchanged rebuild ends as a
  restore rather than a skip.

## Summary of architectural recommendations

All four highest-leverage refactors are now complete:

1. ~~**Extract a `BuildPolicy` trait from the executor**~~ — **done**.
   `classify_products` delegates per-product skip/restore/rebuild decisions
   to a `&dyn BuildPolicy`. `IncrementalPolicy` implements the current
   logic. Future policies (dry-run, always-rebuild, time-windowed) are a
   single trait impl. See `src/executor/policy.rs`.
2. ~~**Decompose `ObjectStore`**~~ — **done**. `mod.rs` split from 664 →
   223 lines into focused submodules: `blobs.rs` (content-addressed
   storage), `descriptors.rs` (cache descriptor CRUD), `restore.rs`
   (restore/needs_rebuild/can_restore/explain). Existing `management.rs`,
   `operations.rs`, `config_diff.rs` unchanged.
3. ~~**Consolidate config resolution with provenance tracking**~~ — **done**.
   Config fields now carry `FieldProvenance` (user TOML with line number,
   processor default, scan default, serde default). `config show` annotates
   every field with its source. See `src/config/provenance.rs`.
4. ~~**Introduce a `BuildContext` struct replacing process globals**~~ —
   **done**. The three process globals (`INTERRUPTED`, `RUNTIME`,
   `INTERRUPT_SENDER`) are replaced by a `BuildContext` struct threaded
   through the `Processor` trait, executor, analyzers, and remote cache.
   See `src/build_context.rs`.

Entries 3, 7, and 8 are partially addressed — the core issues are resolved
but minor gaps remain (see individual entries above).

Entries 1, 2, 5, 6, 9, 10, 12, 13, 14 are observations about the code's
shape — not necessarily problems to fix, but constraints a new contributor
should understand before making structural changes.

The technical observations (code duplication in discovery helpers, dead
fields in `ProcessorPlugin`, scattered error handling) are recorded in
`suggestions.md` as tactical items.
