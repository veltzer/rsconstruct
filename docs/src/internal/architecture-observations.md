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

### 3. Config defaults are scattered, not composed

Three sources of defaults apply in sequence:
1. Per-processor defaults (e.g. `ruff` → `command = "ruff"`) in a giant
   match-or-registry lookup.
2. Scan defaults (src_dirs, src_extensions) via a separate mechanism
   (`ScanDefaultsData`).
3. User TOML overrides both.

The order matters, but it's encoded across `apply_processor_defaults`,
`apply_scan_defaults`, and the serde deserialization. To understand "what
happens when I leave `command` blank for my custom processor?", you trace
four files.

**Implication:** adding a new kind of default (e.g., "environment-derived"
defaults, or project-level defaults in `~/.config/rsconstruct/`) means
inserting another layer into an already-opaque chain. The right shape would
be a single `ConfigResolver` that applies layers in declared order and lets
you ask it "show me the effective config and where each field came from."
This would also make `rsconstruct config show` (which already exists)
richer — it could annotate each field with its source.

**Load-bearing:** medium. Doesn't shape execution, but shapes every
config-related user interaction.

---

### 4. The executor owns too much policy

`classify_products` in `src/executor/mod.rs` is the brain of incremental
building: it decides skip vs. restore vs. rebuild, computes cache keys,
propagates dep-change invalidation. It knows about:
- The `ObjectStore` (cache lookups)
- `Product` internals (config_hash, variant, output_dirs)
- The `Force` CLI flag
- Topological ordering
- Mtime cache semantics (indirectly via `combined_input_checksum`)

If we wanted a different cache policy — time-based expiry, distributed
cache with fallback, content-addressable pruning — the change lands in the
executor. If we wanted an alternate execution mode — dry-run with
explanations, deterministic simulation, demand-driven — same.

**Implication:** the executor is the unintended home of *every* build
policy. A healthier split would extract a `BuildPolicy` (or `Scheduler`)
trait that the executor consults:
- `fn classify(&self, product, cache) -> Action`
- `fn reason(&self, product, action) -> String` (for `--explain`)

Today that logic is hardcoded. With a trait, alternate policies become
pluggable — a dry-run policy that always returns "skip", a paranoid policy
that always rebuilds, a time-windowed policy that uses a real-time clock.

**Load-bearing:** very high. The executor is performance-critical *and*
policy-central. This is the biggest architectural tension in the codebase.

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

`DepAnalyzer::analyze()` walks existing products and adds *inputs* to them.
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

### 7. Processor instance ↔ typed processor mapping is one-way

A `ProcessorInstance` in the config holds `(type_name, instance_name,
config_toml)`. `Builder::create_processors()` deserializes the TOML and
produces a `Box<dyn Processor>`. Afterwards, the TOML blob is discarded.

You can't go from a running processor back to its declaration. Which
`rsconstruct.toml` section produced this processor? Which other instances
exist of the same type? What config field values did the user explicitly
set vs. inherit from defaults?

**Implication:** features that want to introspect *declarations* — `smart
disable`, `config show` with field provenance, hypothetical "re-run only
processors whose config changed" — can't use the live processor objects.
They have to reparse the TOML. There are effectively two models of the
system (declarations vs. runtime) that don't point at each other.

**A healthier shape** would have processors carry a handle back to their
declaration (even just the raw TOML value), or better, a config-resolution
record showing each field's source.

**Load-bearing:** medium.

---

### 8. Global state in the processor runtime

Three `static` items in `src/processors/mod.rs` back the runtime:
- `INTERRUPTED` — atomic bool set on Ctrl+C.
- `RUNTIME` — a lazy-initialized global tokio runtime for subprocesses.
- `INTERRUPT_SENDER` — broadcast channel for signaling active subprocesses.

This is necessary: subprocesses have to know when to terminate, and the
cleanest way is a process-wide signal. But it has consequences:

- **Tests can't isolate**: parallel tests that want different interrupt
  scenarios share the same flag.
- **The runtime is fixed**: you can't swap tokio for another executor or
  have multiple concurrent build contexts (e.g. a daemon mode serving
  multiple projects).
- **Hidden dependencies**: modules that call `run_command` transitively
  depend on the runtime being initialized. It's not in anyone's function
  signature.

**Implication:** any feature that wants multiple builds in one process
(daemon mode, LSP integration, test harness) has to reckon with these
globals. They'd need to be moved into a `BuildContext` struct passed
through the call chain — a big refactor.

**Load-bearing:** medium. Scoped to the process runtime, but it caps what
you can build on top of the library.

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

### 11. Object store as a multi-responsibility module

The `ObjectStore` handles: blob storage (content-addressed), descriptor
lookup (by cache key), mtime database, config-hash comparison, file
restoration (hardlink/copy/decompress), remote cache integration (partial),
and conflict resolution when two processors produce the same output. It's
~1000 lines of Rust covering very different concerns.

**Implication:** almost any caching change lands here. Remote caching is
partially implemented (`remote_pull` is unused) because extending the
monolith is harder than extending focused modules. A better decomposition
might be:
- `BlobStore` — content-addressed bytes, knows nothing about products.
- `DescriptorIndex` — cache key → blob reference, plus metadata.
- `Restorer` — given a descriptor, materializes files on disk (hardlink
  vs. copy vs. decompress).
- `RemoteBackend` — pure transport layer.

The current monolith mixes these and makes "add a third backend" or "change
how restoration handles permissions" into cross-cutting changes.

**Load-bearing:** very high. Second only to the executor for performance
impact.

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

## Summary of architectural recommendations

If someone were to plan a refactor, the highest-leverage items are:

1. **Extract a `BuildPolicy` trait from the executor** (entry 4) — unlocks
   pluggable caching, demand-driven mode, deterministic simulation, and
   richer `--explain`.
2. **Decompose `ObjectStore`** (entry 11) — enables remote cache
   completion, alternate backends, cleaner restoration logic.
3. ~~**Consolidate config resolution with provenance tracking**~~ — **done**.
   Config fields now carry `FieldProvenance` (user TOML with line number,
   processor default, scan default, serde default). `config show` annotates
   every field with its source.
4. ~~**Introduce a `BuildContext` struct replacing process globals**~~ —
   **done**. The three process globals (`INTERRUPTED`, `RUNTIME`,
   `INTERRUPT_SENDER`) are replaced by a `BuildContext` struct threaded
   through the `Processor` trait, executor, analyzers, and remote cache.

Entries 1, 2, 5, 9, 10 are observations about the shape — not necessarily
problems to fix, but things a new contributor should understand before
making structural changes.

The technical observations (code duplication in discovery helpers, dead
fields in `ProcessorPlugin`, scattered error handling) are recorded in
`suggestions.md` as tactical items.
