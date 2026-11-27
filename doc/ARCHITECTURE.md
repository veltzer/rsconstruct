# RSConstruct: orientation map

Written for an AI agent starting work in this repository. It says where
things are, how a build flows through the code, and which rules bite. It is a
map, not a reference: each section ends with the file or chapter that holds
the detail. The appendix at the end is the 2026-08 architectural assessment
(formerly the root `ARCHITECTURE.md`), kept for history and rationale.
Verified against the source at 0.9.167 (2026-11-16).

When this file and the code disagree, the code is right; fix this file.

## What the project is

`rsconstruct` is an incremental build tool: one binary crate (no `src/lib.rs`,
no workspace), Rust edition 2024, toolchain pinned in `rust-toolchain.toml`.
A project declares *processors* in `rsconstruct.toml`; each processor scans
the project for files, declares *products* (inputs, outputs), and the tool
runs only the products whose inputs, config or tools changed, restoring the
rest from a content-addressed cache.

Size: about 350 `.rs` files under `src/`. About 110,000 of the lines are
`src/engines/` (ports of external linters); the build system itself, outside
`src/engines/` and `src/processor/<type>/`, is roughly 45,000 lines.

Linux only. The repo builds itself: CI runs `rsconstruct build`
against this repo's own `rsconstruct.toml`.

## Other documents, and which to trust

| Path | What it is |
| --- | --- |
| `CLAUDE.md` | The binding rules. Read first; it overrides everything here. |
| `docs/src/` | The mdBook: user docs, plus `docs/src/internal/` for maintainers. |
| `docs/src/internal/README.md` | Reading order and one-line summary of each internal chapter. |
| `docs/src/internal/architecture.md` | The maintainers' architecture chapter. Parts lag the code (see below). |
| `docs/src/internal/processor-contract.md` | What every processor must uphold. |
| `docs/src/internal/coding-standards.md` | Conventions enforced in review. |
| `docs/src/internal/rejected-problems.md` | Findings deliberately rejected. Check before "fixing" something odd. |
| `doc/*.md` | Design notes and plans for individual features (variants, structured errors, batch parallelism, ...). Proposals, not descriptions of current behavior. |
| `plan.md` | A working plan; may be stale. |

Known lag in `docs/src/internal/architecture.md`: it describes discovery as a
fixed-point loop that re-runs every processor, and execution as dependency
"levels". The code has since moved to output routing (`Discovery::PerFile`)
and a ready-tracker scheduler. The sections below describe the code.

Note the two directories: `docs/` is the published book (rumdl-linted, part
of the build); `doc/` holds loose notes and is not linted or published.

## Vocabulary

| Term | Meaning |
| --- | --- |
| processor | A unit of work kind: wraps one tool or one in-process engine. Four types: `checker`, `generator`, `explicit`, `mass_generator`. |
| pname | Full processor name, `processor.<type>.<name>`, equal to its module path. Config header, `-p` argument, cache key, default output dir (`out/processor.generator.tera`). |
| iname | Instance name: a pname, or `pname.<instance>` when a stanza is named (`[processor.checker.clippy.lint]`). Key of the `ProcessorMap`. |
| product | One build unit: `inputs`, `outputs`, owning processor iname, `CacheKey`. Checkers have no outputs. |
| analyzer | Dependency scanner run after discovery; adds input edges to products (C includes, Python imports, tera includes, ...). |
| engine | Pure-Rust port of an external tool under `src/engines/`; the `i`-prefixed processors (`iyamllint`, `ishellcheck`) are their front ends. |
| builtin | A processor whose work is rsconstruct's own code (`is_builtin: true`), as opposed to one that spawns a tool. |
| descriptor | Cache entry (marker, blob or tree JSON) addressed by the descriptor key. |

`docs/src/nomenclature.md` and `docs/src/internal/parameter-naming.md` have
the full lists.

## Source tree

```text
src/
  main.rs              lint config, mod list, run(): CLI dispatch (one match arm per command)
  cli.rs               clap definitions. Every Subcommand enum is alphabetical.
  build_context.rs     BuildContext: tokio runtime, interrupt flag, checksum caches, per-build knobs
  runtime_flags.rs     Process-wide flags from the CLI (OnceLock, immutable after init)

  config/              rsconstruct.toml loading, merge, validation, provenance
    mod.rs             Config, FieldSpec, ProcessorDefaults, ScanDefaultsData, Config::load
    processors.rs      [processor] section, ProcessorInstance, applying defaults
    processor_configs.rs  StandardConfig (fields every processor shares)
    settings.rs        [build], [cache], [pages], [commands]
    validation.rs      schema checks on raw TOML, semantic checks after load
    variables.rs       [vars] substitution (line-preserving, see its header)
    provenance.rs      where each value came from (file:line or default)
    overrides.rs       --iset / --pset
    dependencies.rs    [dependencies] and manifest/lockfile readers
    tests.rs           unit tests, including the processor completeness tests

  registries/          inventory-collected plugin registries
    processor.rs       ProcessorPlugin, all_plugins, parse_name, pname
    analyzer.rs        AnalyzerPlugin

  processor/
    mod.rs             Processor trait, ProcessorType, SimpleChecker, SimpleGenerator,
                       run_command family, discovery and batch helpers
    checker/           73 processors, one file each
    generator/         27 processors; mod.rs has shared discovery helpers
    explicit/          generic.rs only
    mass_generator/    11 processors (cargo, cc, gem, jekyll, linux_module, mdbook, npm, pip, sphinx, zola, generic)

  engines/             in-process ports: actionlint, checkpatch, checkstyle, cpplint, hadolint,
                       luacheck, perlcritic, shellcheck, svglint, tidy, xmllint, yamllint
  analyzers/           DepAnalyzer trait, Analysis; cpp, icpp, markdown, python, sass, tera

  file_index.rs        FileIndex: the one filesystem walk (ignore crate), virtual files
  graph.rs             Product, BuildGraph (dedup, dependency edges, topo sort, validation)
  graph_render.rs      DOT / Mermaid / JSON / text / SVG renderers
  cache_key.rs         CacheKey: the only place cache-key components are composed
  checksum.rs          SHA-256 of files, mtime pre-check cache, combined input checksum

  builder/             command layer: Builder owns Config + ObjectStore + FileIndex
    mod.rs             processor creation, discovery loop, graph construction
    build.rs           plan_build, build, dry_run, status
    clean.rs, graph.rs, product.rs, processor.rs, analyzers.rs, tools.rs,
    doctor.rs, smart.rs, sloc.rs, cache_cmd.rs, add_config.rs, symlink_install.rs
                       one file per command family

  executor/
    mod.rs             Executor, classify_products, ReadyTracker
    execution.rs       coordinator + worker pool, batching, per-product run/cache
    handlers.rs        success/failure bookkeeping
    policy.rs          BuildPolicy, IncrementalPolicy (skip / restore / build)

  object_store/        the cache under .rsconstruct/: blobs, descriptors, restore, trim, remote
  remote_cache.rs      s3 / http / file backends
  db.rs, deps_cache.rs, webcache.rs   redb stores
  orphans.rs           removing outputs no product declares any more

  sandbox.rs           bubblewrap + overlayfs execution model
  read_trace.rs        strace-based undeclared-reads report
  python_env.rs        .venv fingerprint as a cache-key component

  tools.rs             ToolInfo registry (central TOOLS table + per-processor submissions), installers
  tool_lock.rs         rsconstruct.lock
  tool_store.rs        .rsconstruct/tools, hard-linked from a machine-wide store
  download.rs          every network download (the only place curl is invoked)

  output.rs            the sink for human-readable output (keeps --json clean)
  json_output.rs       JSON Lines build events
  tables.rs            every CLI table, as named TableDefs
  platform.rs          all OS-specific calls
  errors.rs, exit_code.rs, color.rs, display.rs, progress.rs, stats.rs,
  phases.rs, watcher.rs, word_manager.rs
```

Outside `src/`:

```text
tests/main.rs          single integration-test binary
tests/common/mod.rs    helpers (temp project, run_rsconstruct, JSON result parsing) and test_checker!
tests/tests_mod/       per-command tests (build, cache, incremental, graph, tools, watch, ...)
tests/processor/<type>/<name>.rs   one file per processor, mirroring src/processor/
docs/src/processor/<type>/<name>.md  one page per processor
scripts/               ci-install-tools.sh (fleet-shared), ci-install-tools.repo.sh (this repo), table generators for engines
rsconstruct.toml       this repo's build; rsconstruct.lock pins its tools
.github/workflows/ci.yml  fleet-shared, byte-identical across rs* repos
```

## How a build runs

Entry: `main.rs::run()` parses the CLI, initializes `runtime_flags`, creates
the `BuildContext`, installs the Ctrl+C handler, then dispatches on
`Commands`. For `build`:

1. **`Builder::new_with_overrides`** (`builder/mod.rs`).
   `Config::load` merges the user config's `[build]` under `rsconstruct.toml`,
   after `[vars]` substitution; validates;
   runs post-config hooks (`phases.rs`). The builder then opens the
   `ObjectStore` and builds the `FileIndex`. This is the only filesystem
   walk. It honors `.gitignore` and `.rsconstructignore`, and never walks
   `.rsconstruct/` or configured output roots unless a `src_dirs` entry
   names one.

2. **`plan_build`** (`builder/build.rs`): `create_processors` builds one
   `Box<dyn Processor>` per declared instance through its plugin's `create`
   function, with defaults already applied to the TOML. Only declared
   processors exist; nothing is enabled implicitly.

3. **Graph construction** (`build_graph_with_processors_impl`), in phases
   that `--phases` times and `--stop-after` can cut short:
   - *discover*: `discover_products`. Round 0 runs every active processor
     over the full index. The outputs it declared become virtual files, and
     later rounds hand them on: a `Discovery::PerFile` processor sees an
     index of only the new files, a `Discovery::WholeIndex` processor (the
     default) rediscovers over everything. Stops when a round adds no
     product; still adding at `[build] max_discovery_passes` is an error.
     `BuildGraph` deduplicates re-declarations.
   - *add_dependencies*: analyzers add inputs to products, cached in
     `deps.redb`. Skipped for `clean`.
   - *tool_version_hashes* and *python_env*: more `CacheKey` components.
   - *resolve*: `resolve_dependencies` links a product to the producer of
     each of its inputs. This is the only ordering mechanism; there are no
     explicit ordering primitives.
   - *validate*: `graph.validate` against `[graph]` config.

4. Back in `plan_build`: `-p`/`-x` selection (pulling in producers), the
   required-tools check for processors that actually have products, the
   optional lock verification, and target filtering.

5. **Classify** (`executor::classify_products`): an up-front prediction of
   skip / restore / build per product. It sizes the progress bar and prints
   the counts; it does not decide anything.

6. **Execute** (`executor/execution.rs`). A coordinator thread owns a
   `ReadyTracker`; a pool of exactly `-j` workers runs units. A product is
   ready the moment its last dependency finishes. At dispatch its input
   checksum is taken and the `BuildPolicy` decides skip, restore or build,
   so a dependency that rebuilt to identical bytes leaves it skippable.
   A unit is one product or one batch chunk. Two scheduling exceptions:
   - a batch-capable *checker* is held until all its products are ready,
     then runs as one batch;
   - dependents of a *mass generator* product wait for every product of
     that instance, because one tool run rewrites all their files.

   Per product: remove stale outputs, run `execute` or `execute_batch`,
   then store outputs as blobs plus a descriptor. If analyzers scanned a
   file a product just regenerated, `Analysis::reanalyze` refreshes the
   graph mid-build.

7. After the run: flush the mtime cache, remove orphan outputs (full builds
   only), print the summary, map failures to an exit code (`exit_code.rs`).

Other commands reuse pieces of this through `Builder`: `clean` builds the
graph in `ForClean` mode (no analyzers), `status` and `--dry-run` share
`plan_build`, `graph`, `product show` and `processor files` build the graph
and read it.

Detail: `docs/src/internal/output-routing.md`,
`cross-processor-dependencies.md`, `processor-ordering.md`.

## Processors

The central abstraction. Three pieces, all in the processor's one file:

- **`ProcessorPlugin`** (`registries/processor.rs`), submitted with
  `inventory::submit!`. Static metadata and the factory: `name`,
  `processor_type`, `version`, `create`, `fields` (the schema, as
  `FieldSpec`s), `omit_standard_fields`, `scan_defaults`, `defaults`,
  `keywords`, `description`, and capability flags `is_builtin`, `is_rust`,
  `supports_batch`, `caches_outputs`, `sandboxable`, `max_jobs_cap`.
  Metadata is read from here, never from a throwaway instance.
- **`Processor` trait** (`processor/mod.rs`). Required: `scan_config()` and
  `execute(ctx, product)`. Defaulted: `discover`, `discovery`,
  `discover_for_clean`, `extra_dep_inputs`, `clean`, `auto_detect`,
  `required_tools`, `execute_batch`, `config_json`, `replaces_own_outputs`.
- **`ToolInfo`** (`tools.rs`), one submission per tool only this processor
  runs. Shared runtimes and config-selected tools stay in the central
  `TOOLS` table; a test enforces the split.

Most processors do not implement the trait themselves:

- `SimpleChecker` + `SimpleCheckerParams`: a tool run over files. Template:
  `src/processor/checker/svglint.rs` (about 40 lines).
- `SimpleGenerator<C>` + `SimpleGeneratorParams`: one input to one output
  with a custom `execute_fn`. Template: `src/processor/generator/sass.rs`.
- Own struct: custom discovery or prediction (`clippy`, `script`, every mass
  generator). `mass_generator/zola.rs` is the reference shape for a tool
  that fills a directory.

The four types differ in product shape and caching:

| Type | Products | Cache entry |
| --- | --- | --- |
| checker | one per file, no outputs | marker (the pass itself) |
| generator | one per input (per format) | blob per output |
| explicit | one per stanza, declared `output_files` / `output_dirs` | tree |
| mass_generator | one per predicted output file; the tool runs at most once per manifest per build | blob per file |

There is no creator type and no Lua processor type; both were removed.

`StandardConfig` (`config/processor_configs.rs`) carries the fields every
processor shares: scan settings (`src_dirs`, `src_extensions`, `src_files`,
excludes), `command`, `args`, `dep_inputs`, `dep_auto`, `output_dir`,
`batch`, `max_jobs`, `enabled`, sandbox fields.

### Adding a processor

1. `src/processor/<type>/<name>.rs` with the impl (or Simple* params), the
   config struct if it has custom fields, the plugin entry, and its
   `ToolInfo`.
2. `mod <name>;` in `src/processor/<type>/mod.rs`. Omitting it compiles as
   dead code; `every_processor_file_is_declared` catches it.
3. `docs/src/processor/<type>/<name>.md` and its `docs/src/SUMMARY.md` entry.
4. `tests/processor/<type>/<name>.rs`, registered in `tests/main.rs`.

`every_plugin_has_docs_and_tests` in `src/config/tests.rs` enforces the docs
page and the test file (a short allowlist covers older processors without
tests; do not add to it).
Every tunable is a config field with a `FieldSpec`; check with
`rsconstruct processor defconfig <name>`. Bump the plugin `version` when
output, discovery or field interpretation changes
(`docs/src/internal/processor-versioning.md`).

### Running subprocesses

All child processes go through `run_command`, `run_command_capture`,
`run_command_with_timeout` or `run_command_with_stdin` in
`processor/mod.rs`. That one inner function provides interrupt handling
(tokio `select!` against the watch channel, `kill_on_drop`), command
logging, the debug assertion that a processor runs only tools it declared,
sandbox wrapping and read tracing. Two documented exceptions spawn by hand:
tool installation (needs the terminal) and opening a viewer (must outlive
rsconstruct). No shell is ever used
(`docs/src/internal/no-shell-policy.md`). Downloads go through
`download.rs` only.

## Caching

State lives in `.rsconstruct/`:

| Path | Content |
| --- | --- |
| `objects/` | blobs by SHA-256 of content, two-hex sharding, optionally zstd |
| `descriptors/` | marker / blob / tree JSON by descriptor key |
| `db.redb` | stored processor configs (for the "Config changed" diff), last tree per product |
| `deps.redb` | analyzer results per `(analyzer, source)` |
| `mtime.redb` | `(path, mtime)` to checksum pre-check |
| `webcache.redb` | HTTP responses with TTL |
| `outputs.json` | record used for orphan-output removal |
| `tools/` | project-local tool installs |

Descriptor key = hash of processor name, plugin version, `CacheKey` digest,
and combined input *content* checksum. Input paths enter the key only when
`output_depends_on_input_name` is set, so a rename is normally a cache hit.

`CacheKey` (`cache_key.rs`) is an ordered list of named components:
`config` (the processor's checksum fields: standard ones plus every
`FieldSpec` with `affects_output: true`), `variant`, `analyzer`, `tool`,
`python_env`. All contributors go through `CacheKey::push`. Do not build key
material anywhere else. `rsconstruct product show <path>` prints the
components, which is the way to debug an unexpected rebuild or an
unexpected hit.

Restore is by hardlink or copy (`[cache] restore_method`); restored
hardlinks are read-only, which is why the executor unlinks a product's
outputs before rebuilding. Remote cache backends plug into `ObjectStore`.

Detail: `docs/src/internal/cache.md`, `checksum-cache.md`,
`dependency-caching.md`, `python-env-fingerprint.md`, `orphan-outputs.md`,
`shared-output-directory.md`.

## Execution models

`[build] execution_model` (overridable per processor) is `native` or
`sandbox`. Sandboxed, each tool invocation is wrapped in bubblewrap with an
overlayfs over the project: writes to declared outputs are applied on
success, any other write fails the product. It is a correctness check, not
a security boundary. A sandboxed *builtin* processor has no tool to wrap,
so the executor re-invokes rsconstruct itself under bwrap through the
hidden `sandbox-run` command (`sandbox::execute_builtin`).

`[build] undeclared_reads = "report"` runs tools under strace and warns
about project files read but not declared (`read_trace.rs`).

Detail: `docs/src/internal/sandbox.md`, `sandbox-chromium.md`,
`undeclared-reads.md`.

## Tools

- Registry: `tools::all_tools()` = central `TOOLS` + per-processor
  `ToolInfo`. `name` is a bare binary name passed to `which`; never a path.
- `rsconstruct tool install project` puts tools in `.rsconstruct/tools` at
  the versions `rsconstruct.lock` names, hard-linked from a machine-wide
  store (`tool_store.rs`). `tool install machine` is the last resort
  (system packages).
- Python tools are not installed by rsconstruct: they belong to the repo's
  `.venv`, owned by uv.
- Tool identity (path, size, mtime of the resolved binary) is hashed into
  the cache key when `[build] hash_tool_versions` is on.

Detail: `docs/src/tool-lock.md`, `docs/src/internal/download-policy.md`.

## Cross-cutting rules that are easy to break

These come from `CLAUDE.md` and the internal docs; the code will not always
stop a violation.

- **No macros** in `src/`. No `#[cfg(...)]` anywhere. OS calls only in
  `platform.rs`.
- **Lints are errors**: `#![deny(warnings)]`, `clippy::all` denied,
  pedantic and nursery on. There is no crate-level allow list at all
  (`[lints.clippy]` in `Cargo.toml` only sets levels); fix the code, do not
  suppress the lint.
- **Error context** on every IO `?` (`.with_context(...)`). `errors::ctx()`
  is `#[track_caller]`.
- **Strict, no fallbacks**: a missing tool, a named config file that does
  not exist, an unknown config field are all errors. Never try-and-fall-back.
- **Human output goes through `output.rs`** (`info`, `detail`, `warn`,
  `error`), so `--json` stays machine-readable. Tables go through
  `tables.rs`.
- **Paths are relative to the project root** everywhere: products, index,
  cache entries. The process cwd is the project root.
- **Determinism**: discovery, processor iteration and topo sort are sorted.
- **CLI subcommand enums are alphabetical** by kebab-case name.
- **A command invoked without its required subcommand lists the choices**,
  one line each and no flags (the user's global rule for every CLI). Keep
  it true when adding a command group to `cli.rs`.
- **No back-compat obligations**: there are no external users; changing an
  on-disk format or a config name is fine, clean-and-rebuild is acceptable.
- **`ci.yml` and `scripts/ci-install-tools.sh` are fleet-shared and
  byte-identical.** Never fix a failure there. Repo-specific installs go in
  `scripts/ci-install-tools.repo.sh`; new checks go in `rsconstruct.toml`.
- **CLI renames reach the fleet**: other repos' CI calls `rsconstruct tool
  ...` and friends, so a rename needs a release before the fleet follows.

## Working in the repo

The gate is the repo's own build, the same command CI runs:

```sh
rsconstruct build
```

It runs, as processor instances in `rsconstruct.toml`: `cargo build` (dev),
`cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`,
`cargo deny check`, `cargo nextest run`, and rumdl over `README.md` and
`docs/src`. CI first replaces the installed `rsconstruct` with a build of
the checkout (`ci-install-tools.repo.sh`), so the gate tests the code under
change; locally use a binary built from the working tree for the same
effect.

Things to know while iterating:

- `cargo test` with no filter is the rule; never skip a failing test.
  Integration tests are one binary (`tests/main.rs`) that runs the built
  `rsconstruct` in temp projects. Tests needing an external tool call
  `require_tool`.
- Unit tests live beside the code in `src/` (`#[cfg(test)] mod tests`),
  because there is no library crate for `tests/` to link against.
- After `cargo release`, `cargo build` can serve a stale binary;
  `cargo clean -p rsconstruct` fixes it.
- `/tmp` is a RAM tmpfs on the dev machine; cap heavy profiling or builds
  with `systemd-run` and `MemoryMax`.
- A warm rsconstruct build runs only changed products. To validate a large
  change, build cold (fresh clone or `rsconstruct clean all`).
- Releases are cut with `cargo_release_patch.sh`; CI builds the binaries
  from the `chore: Release` commit (`docs/src/internal/crates-io.md`,
  `docs/src/binary-releases.md`).
- Tasks touching repos under `~/git` end with `rsmultigit check same`
  clean.

## Where to look for a given task

| Task | Start at |
| --- | --- |
| New or changed processor | its file in `src/processor/<type>/`; template `checker/svglint.rs` or `generator/sass.rs` |
| New config field on a processor | `fields: &[FieldSpec]` on its plugin, plus its config struct |
| New `[build]` / `[cache]` setting | `config/settings.rs`, then `docs/src/configuration.md` |
| Unexpected rebuild or stale hit | `rsconstruct product show <path>`, `--explain`; then `cache_key.rs`, `executor/policy.rs`, `object_store/restore.rs` |
| Product not discovered / wrong inputs | `file_index.rs` (`scan`, `query`), the processor's `discover`, `builder/mod.rs::discover_products` |
| Wrong build order | `graph.rs::resolve_dependencies`; ordering comes only from input/output matching |
| Scheduling, batching, `-j`, `max_jobs` | `executor/mod.rs` (`ReadyTracker`), `executor/execution.rs` |
| Missing dependency edge (include, import) | `src/analyzers/<lang>.rs`, `deps_cache.rs` |
| New CLI command or flag | `cli.rs` (alphabetical), dispatch in `main.rs::run`, body in `builder/<family>.rs`, then `docs/src/commands.md` |
| Table columns or layout | `tables.rs` (`defs`) |
| Tool install, lock, detection | `tools.rs`, `tool_lock.rs`, `tool_store.rs`, `builder/tools.rs` |
| Port of an external linter | `src/engines/<tool>/`, front end in `src/processor/checker/i<tool>.rs`; parity is checked by differential runs against the real tool |
| Sandbox failure | `sandbox.rs`, the processor's `sandboxable` and `ToolInfo.sandbox_writable` |
| Exit codes, error classes | `exit_code.rs` |
| Watch mode | `watcher.rs` |
| Why something is the way it is | `docs/src/internal/rejected-problems.md`, `architecture-observations.md`, the appendix of this file |

## Appendix: architectural assessment (2026-08)

History and rationale, not a map: six structural deficiencies found by a
full-codebase review on 2026-08-05, with STATUS notes where they were
addressed. Line counts, file names and module layout below are as of that
date; the sections above describe the code as it is now.

Full-codebase architecture review, 2026-08-05. This replaces the retired
`ARCHITECTURE_FINDINGS.md` (a bug-level audit whose items were remediated
through 2026-08-04; its record lives in git history). This assessment is
deliberately different in kind: it records **fundamental architectural
deficiencies** — properties of the design that generate whole classes of
bugs — not individual defects. The bug-level findings that led here are
cited as *symptoms as evidence*, not as the point.

Method: five parallel deep scans through distinct lenses (module dependency
topology; build-pipeline data flow and state lifecycle; the processor
extension model; the persistence/caching model; the command/driver layer),
followed by hand-verification of the load-bearing claims. Scale at time of
review: 169 `.rs` files, ~39,000 lines, 33 flat top-level modules, single
binary crate, ~93 processors.

The six deficiencies are ordered by how much of everything else each one
causes. §1 and §2 are generators — most recorded bug history is downstream
of them. §7 explains the mechanism and ranks the fixes by leverage.

---

### 1. Per-processor knowledge has no single source of truth — and that IS the module tangle

#### The claim

A processor is the system's unit of extension. Describing one completely
requires **~14 edit sites across 8+ files**, of which the compiler enforces
roughly 4. There is no schema from which the rest derive; each site is a
hand-maintained projection of the same facts.

#### The evidence

The touch-points for one processor:

1. The processor file itself + `inventory::submit!`
2. The `mod` declaration in its category's `mod.rs` — **a silent kill
   switch**: omit it and the file compiles as dead code, submits nothing,
   and the processor simply does not exist, with no diagnostic
3. Config struct in `config/processor_configs.rs` (if any custom field)
4. `#[serde(default = "fn")]` + free `default_*()` functions — default
   value encoding #1
5. `impl Default` — default value encoding #2 (same values, second syntax)
6. `known_fields()` — field name list #1
7. `checksum_fields()` — field name list #2
8. `must_fields()` — field name list #3
9. `field_descriptions()` — field name list #4
10. `scan_defaults_for` in `config/mod.rs` — a **92-arm** name-keyed match
11. `processor_defaults_for` in `config/mod.rs` — a **71-arm** match —
    default value encoding #3
12. `expected_field_type` in `config/mod.rs` — a **~120-arm** match on
    `(processor, field)`
13. `TOOLS` registry in `src/tools.rs` (83 entries)
14. Docs page, `SUMMARY.md` entry, test file, `tests/main.rs` registration

Every config field is therefore declared up to **six times** (struct+serde,
Default impl, known/checksum/description lists, type table), and every
default value **twice in two encodings** (Rust `Default` for serde; TOML
data in `processor_defaults_for` for the provenance system). `MarpConfig`
carries an in-source comment admitting the two encodings are
hand-synchronized *and that a divergence already regressed once*. The
14-name `StandardConfig` field block is copy-pasted across ~36
`KnownFields` impls.

The match tables' `_ => None` fallthrough is a **legal, silent answer** — a
missing arm is indistinguishable from a deliberate "no defaults here"
(which is correct for `script`, `creator`, `explicit`). So the compiler
cannot help, and git history proves the cost is not just high but
*unpredictable*: commit `fbaa7ac` added `prettier` in exactly one file — it
compiled, linked, registered, and shipped **silently mis-configured** (no
scan defaults, no processor defaults, no TOOLS entry); the remaining
registries were filled in by later commits, discovered by symptom.

The system's own tests confess the diagnosis. `config/tests.rs` carries
~150 lines of whole-registry consistency checks
(`every_known_field_has_an_expected_type`,
`every_expected_type_arm_is_a_live_field`, …) — **a schema checker written
by hand, at runtime, in the test harness, because there is no schema**. One
of them must verify its property backwards, with the comment: *"The table
is a match, not data, so it can't be enumerated directly."*

#### Why this is also the dependency knot

The crate's central cycle — `config → registries → processor → config` —
exists *because of* this duplication. Measured by reference sites:
processor→config 679, processor→registries 638; `config/mod.rs:483`
imports the registry while `registries/processor.rs` imports both
`processor` (for the `create` fn pointer and `ProcessorType`) and `config`
(for `KnownFields`). The 163 hardcoded processor-name arms in `config` are
per-processor knowledge that could not be expressed on the plugin entry, so
they landed in `config`, which therefore must know the registry, which must
know processors, which must know config. **These three modules
(~7,500 lines: the config schema, the registry, all 93 tool integrations,
and the process runner) are one indivisible unit** — a workspace split is
impossible until this is resolved. The docs' claim ("no central registry
table to update", `architecture.md:323`) is true of the construction table
`inventory` eliminated and false of the three metadata tables that
replaced it.

#### The honest caveat

Two of the project's own rules feed this deficiency:

- **"No macros"** blocks the `#[derive]`-style unification that would let a
  single per-field `FieldSpec { name, type, default, affects_output,
  required, doc }` table generate all six projections. Without a derive,
  the spec table cannot generate struct fields and serde attributes; you
  either keep both (partial win) or lose static typing at every
  `self.config.x` site.
- **"Never create dummy instances"** forces metadata onto the static plugin
  entry, which is what pushed the config-dependent knowledge into `config`'s
  match tables (see also §3).

Both rules are defensible. Their combined price is this deficiency, and the
price is currently paid silently.

#### Fix shape for problem 1

The H1-shaped refactor: a data/constructor split of the plugin entry that
absorbs the three match tables into the plugin file (defaults and scan
defaults as `toml::Value` data — the provenance system needs data anyway),
plus deriving `known_fields` by union with `StandardConfig`, flipping
`checksum_fields` to in-by-default/opt-out, and — cheap, immediate,
schema-less — **a processor-level completeness test**: iterate
`all_plugins()` and assert each has a defaults arm or explicit opt-out, a
docs page, a `SUMMARY.md` entry, and a test file. That one test closes the
`prettier` failure mode today.

> **STATUS (2026-08-05): DONE.** A processor is now one file. `FieldSpec`
> on the plugin entry is the schema (known/checksum/must fields,
> descriptions, and expected types all derive from it); the three match
> tables are deleted; every default value is encoded exactly once (the
> marp double-encoding class is structurally gone); config structs live in
> their processor files (`processor_configs.rs`: 1,996 → 281 lines, now
> only `StandardConfig` + shared types + analyzer configs); the tool
> registry accepts per-file `ToolInfo` submissions; completeness tests
> enforce the mod declaration, docs page, and test file (with allowlists
> for grandfathered gaps). The five hand-written schema-consistency tests
> were deleted as structurally impossible to violate. Remaining from this
> section: the `config↔registries↔processors` import cycle still exists
> (the *knowledge* moved; the module boundaries didn't), and docs pages
> are still hand-written — the config tables in them can drift until
> generated or verified.

---

### 2. Facts are re-derived instead of passed — the build has no materialized identities

#### Input content identity is never data

`Product.inputs` is `Vec<PathBuf>` — a list of *names*. The build's answer
to "what content is in these files" exists only as transient `String`s
recomputed from live disk **three to five times per product per build**:

1. `classify_products` (`executor/mod.rs`) — stored in
   `ClassifiedProduct.input_checksum`
2. `unlink_pending_outputs` → `remove_stale_outputs` — uses checksum #1 to
   select files for **deletion**
3. `prepare_level_work` (`executor/execution.rs`) — recomputes, with a
   15-line comment on why reusing #1 is wrong
4. `handle_restore` — uses #3
5. `handle_success` — recomputes **again** post-execution, with a 14-line
   comment on why reusing #3 is wrong

Each site is individually correct and each comment genuinely illuminating.
Together they state what the code cannot: **the descriptor key is not a
property of a product; it is a property of a product at an instant**, and
the system needs three different instants (pre-unlink, pre-execute,
post-execute) to be correct — with nothing but comments to tell a
maintainer which instant they are standing in. The doc comment on
`Executor::execute` claims checksums are reused from classification; the
same file recomputes them 600 lines later. Nobody owns the question.

Corollary machinery that exists solely to keep independent observations of
one fact in agreement: the `forget_in_session` eviction protocol (three
manual call sites), the separate `checksum_output` function (one cache
serving two populations — stable inputs, volatile outputs — distinguished
only by which function the caller picked), the 2-second
recently-modified mtime heuristic, and the `MISSING:{path}` sentinel — an
absence marker that is a perfectly valid `String`, type-indistinguishable
from a real content hash, flowing into `descriptor_key()`.

#### Cache-key completeness is unowned

`CacheKey` (typed components, length-prefixed, attributable) solved key
*composition* — but contribution remains fully distributed: Config at
`Product::new`, Variant at `with_variant`, Analyzer pieces mid-pipeline,
ToolVersion via `apply_tool_version_hashes` — **a post-hoc mutation pass
over the whole graph that any caller can skip undetected**.
`CacheKey::new()` is a valid, empty, under-keyed key, indistinguishable
from a legitimately component-free product. That is exactly how the
"tool upgrades leave stale PASSes valid" bug hid: an absent contributor is
invisible. Component *order* is documented as significant and enforced by
nothing — reordering two calls in `build_graph_with_processors_impl`
silently cold-starts every cache in every project.

#### Product state has no representation

There is no `ProductState`, no written state machine. Four overlapping
enums exist (`ProductAction`, `ExplainAction`, `PreCheckResult`,
`RestoreOutcome`) plus the actual runtime state: membership in
`failed_products`/`failed_processors` HashSets, where "skipped because a
dependency failed" and "failed during execution" are the same state and are
counted together. `Classification` — the phase output shaped like a plan —
is a checkpoint the executor refuses to honor: its `input_checksum` field
is written and never read, its `action` is consumed as a boolean, and
`prepare_level_work` re-derives the rebuild decision by calling the object
store directly, **bypassing the `BuildPolicy` trait that was extracted to
own that decision**. Consequently `status`, `--dry-run`, the
"`N to build`" banner, and the executor answer "what will happen to this
product" through three independent code paths — **the plan shown to the
user is structurally not the plan that executes.**

#### The graph undermines its own indices

`BuildGraph` owns four indices derived from product contents (`interner`,
`output_to_product`, `input_to_products`, `checker_dedup`) and also hands
out `get_product_mut`. Analyzers extend `product.inputs` through it;
the indices are never updated. `resolve_dependencies` then does
`interner.get(input)?` — the `?` **silently skips un-interned paths**, so
an analyzer-added input that is another product's output produces no
dependency edge and no diagnostic. Graph-structure invariants were
progressively hardened into construction (`register_product`, output
conflict checks — good, deliberate work); the mutable back door walks
around all of it on every build.

#### Fix shape for problem 2

`CacheKey` is the in-repo proof of the cure — it turned an opaque string
assembled at five uncoordinated sites into typed, ordered, attributable
data. Apply the same move three more times: a `ResolvedInputs` value
(path → `InputDigest::{Content(hash), Absent}`) produced once per product
at a defined instant and passed forward; a `CacheKey` that cannot be
`digest()`ed until every contributor has had its turn (typed builder /
completeness token, components appended in `Component` order which is
already `Ord`); a single `ProductState` consumed by classify, executor,
`status`, and `--explain` alike. Seal `get_product_mut` behind an
`add_inputs(product_id, paths)` method that maintains the indices.

---

### 3. Capabilities live on the link-time registry entry, so config-dependent capabilities have no home

`supports_batch`, `caches_outputs`, `max_jobs_cap`, `version`, `is_builtin`, and
all four field-metadata accessors are `&'static` fields on
`ProcessorPlugin`, registered via `inventory::submit!` at link time.
Anything not compiled into the binary therefore **cannot have
capabilities**. (This section used to be about the Lua plugin host, which
got the execution trait and none of the metadata registry — no cache
versioning, no field validation, no multi-instance sections, no
completion — and whose scripting runtime downgraded real errors to
warnings. The Lua processor type was removed on 2026-10-17: nothing in the
fleet used it. What remains is the builtin half of the problem.)

The sharpest instance was the `fix` capability: the `script` processor
declared `can_fix: false` beside a working `fix()` implementation,
reconciled by `registries::can_fix(name) || processors[name].config_has_fix()`
at the call site, because the static flag could only lie about a
config-dependent capability. The `fix` command and its hooks have since been
removed, so that example is gone, but the boundary it exposed is not: the
flag for a capability lives on the registry, and nothing gives a capability
that depends on configuration a home of its own.

This is not neglect; it is forced by pairing link-time registration with
the no-dummy-instances rule. Link-time registration itself is a good trade
for this tool (no central construction table, metadata without
instantiation, whole-registry tests, static clap completion); the cost is
that a capability a processor only has under some configuration has to be
answered twice.

#### Fix shape for problem 3

When a capability next depends on configuration, give it one query on the
configured processor that consults the registry flag, instead of an ad-hoc
`||` at each call site, so capability queries have exactly one home.

---

### 4. There is no command layer — fifteen drivers hand-roll subsets of one pipeline

There is exactly one canonical pipeline (discover → analyze → resolve →
validate → classify → execute) and **fifteen command entry points** that
re-implement prefixes of it by hand: three independent discovery loops
(`build_graph_with_processors_impl`, `build_graph_filtered`, an inline copy
in `analyzer build`), three independent "will this rebuild?"
implementations (executor policy, `print_product_status`,
`valid_cache_keys`), and one hand-rolled filesystem walk
(`clean unknown`). The drift is documented in the code itself: the
`analyzer build` copy shipped skipping resolve+validate, reported success
on configs `build` rejects, was patched — and remains a copy.

The sharpest evidence that this is architectural: **interruptibility is a
property of the executor, not of the process.** `is_interrupted()` polling
lives in `executor/` (plus watch and the subprocess runner). `clean
unknown` and `analyzer build` poll it zero times — any driver that
skips the executor silently loses Ctrl+C.

`Builder` is the non-abstraction at the center: three fields (`config`,
`file_index`, `object_store`), **31 public entry points across 10 files**,
no build state, everything re-derived per call (`create_processors()` at 14
sites). It is a service locator wearing a struct — `Builder::doctor()`,
`Builder::sloc()`, `Builder::product_show()` do not build anything. The
dispatch knowledge ("which commands need config") is split three ways:
20 hand-placed `Builder::new` calls in `main.rs`, ad-hoc
`rsconstruct.toml` existence probes in three places, and
`unreachable!("handled in main.rs")` arms inside `Builder` methods that
take whole action enums and declare half the variants impossible — a
runtime panic standing in for a compile-time property.

The testability receipt: `Builder::new` performs three unmockable side
effects (require config in CWD, open redb, walk the filesystem), with no
constructor accepting pre-built parts. **`src/builder/` has 5 unit tests
against 31 entry points** (all for one pure free function), versus 67 in
`src/config/`. All behavioral coverage is integration tests spawning the
binary. The architecture is selecting which code gets tested.

Output is the same absence viewed from the other side. `output.rs` is a
correct reporter with an honest charter, enforced by a scanner test — over
a **hardcoded 9-file allowlist** (`executor/`, `object_store/`). Outside
it: ~360 raw `println!` sites, 73 `is_json_mode()` call sites (versus 10
uses of the correct combined predicate), and one fact — "product X
failed" — crossing **eight independent formatting/suppression decision
points** between the executor and the terminal. Two output regimes coexist
in one binary, and the guard test is green precisely because the broken
half is outside its file list (`status --json --verbose` and
`watch --json` both corrupt the JSON stream today). Mode flags are threaded
by four different mechanisms (`OnceLock` globals, a hand-threaded `verbose`
parameter in 72 signatures, `BuildOptions` fields, a `BuildContext`
setter), and `--quiet` appears in zero tests.

#### Fix shape for problem 4

Not more extraction — `cache_cmd.rs`'s header records that extracting the
biggest match arm just relocated the pattern. Two structural moves:

1. **A `Command` abstraction** owning config acquisition, pipeline access,
   interrupt polling, and result rendering, so the fifteen drivers become
   fifteen policies over one driver and a sixteenth cannot silently opt out
   of Ctrl+C. Prerequisite: `Builder` (or its successor `Project` value)
   accepts its three dependencies instead of constructing them, which is
   the single change that makes the command layer unit-testable.
2. **A data/presenter split**: commands return a serializable result value;
   one presenter chooses json / verbose table / compact table / suppressed.
   This deletes the copy-pasted `if json {…} else if verbose {…} else {…}`
   triple at ~30 sites, defines `--json --verbose` for free, and lets the
   bare-println scanner become universal instead of allowlisted.

---

### 5. There is no persistence layer — seven stores share a directory name and nothing else

`.rsconstruct/` is a naming convention, not a layer. Seven mechanisms
(`objects/`, `descriptors/`, `db.redb`, `mtime.redb`, `deps.redb`,
`webcache.redb`, plus out-of-directory state: `.tools.versions`, words
files, tags state) have no common open/verify/repair lifecycle, no shared
path constants, and **four mutually contradictory corruption policies**:

- `cache trim` **fails closed** on an unparsable descriptor (correct in
  isolation — skipping would GC live blobs);
- `get_descriptor` **fails open silently** on the same descriptor (a
  permanent, invisible cache miss);
- `db.rs` **fails loud** ("run cache clear") and, despite its function
  name, never recreates;
- a corrupt `.tools.versions` **fails the whole build**.

Net effect: one corrupt descriptor makes `build` silently rebuild forever
while `cache trim` refuses to run, and the only repair is `cache clear` —
`remove_dir_all` on the whole directory, destroying five unrelated stores.
The schema-versioning inversion completes the picture: the **descriptor
format — the on-disk and over-the-wire contract shipped between machines
via the remote cache — has no version field**, while `webcache`, the one
freely-re-fetchable store, is the only versioned one (`webcache_v2`).
Symptomatic of the missing inventory: `config/mod.rs` documents a
`toolver.redb` that does not exist.

**Concurrency is unresolved in both directions.** The design is
single-process by enforcement (four redb exclusive locks; a test-only
`db_name` parameter exists because two stores cannot open one directory)
yet pays multi-process costs throughout (pid-tagged temp names,
rename-race recovery branches, uuid tempfiles in the remote backend) —
and one function's comment justifies including temp files with a
single-process argument fifty lines from pid-tagging that assumes the
opposite. Committing to single-process (one flock at startup) deletes real
code from the two hottest write paths; committing to multi-process unlocks
daemon mode and shared CI caches. The current position pays for both and
guarantees neither.

**The remote cache is a transport, not an abstraction.** `RemoteCache` is
four methods with zero pinned semantics: error-vs-absence is decided per
backend (HTTP rigorously distinguishes them; S3's `exists` folds
credential failure into "absent"; File folds permission errors into
"absent"), atomicity is per-backend folklore, and there is no conformance
suite — `HttpBackend` has zero tests, and the good round-trip tests all
run against `FileBackend` only. The write-only-remote and torn-write bugs
fixed in 2026-08 could exist *because* nothing pins the contract.

#### Fix shape for problem 5

A `Store` trait with declared schema version and an open/verify/repair
lifecycle; one policy for absence, one for corruption, per-store repair so
`cache clear` stops being the only tool. A one-line decision — "rsconstruct
is single-process per project, enforced by one lock file" — followed by
deleting the multi-process scaffolding. A backend conformance test suite
(atomic overwrite, error-vs-absent, integrity of fetched descriptors) that
every `RemoteCache` impl must pass.

---

### 6. The cache model's core assumption — product ≡ invocation ≡ result ≡ positive output set — is false three ways

- **Batching breaks product ≡ invocation.** The cache is strictly
  per-product; execution is per-chunk. `execute_checker_batch` fans one
  exit status out to N products, so one bad file in a 50-file ruff chunk
  denies cache entries to 49 clean files on every build until it is fixed.
  Batching silently trades cache granularity for speed, and nothing in the
  types marks the trade (the per-file variant exists but is the minority
  path).
- **Outputs-as-inputs breaks product ≡ result.** A product's output is
  another product's (or its own) input. The entire eviction/recompute
  apparatus of §2 exists because the model assumed content is stable for
  one run and the build itself violates that assumption by writing files.
- **Trees are positive-only manifests.** A tree descriptor records what
  should exist, never what should be absent. Restore writes entries and
  deletes nothing; deletion lives only on the build path
  (`previous_tree_paths` before execution). So build-then-build converges
  while restore-then-restore leaves orphans, and a shrunk output set
  survives forever once restored.

Each shortfall is being addressed (or slated) as a per-processor patch flag
— `output_depends_on_input_name` is the acknowledged fourth instance of the
same shape. The pattern says the model's notion of product identity is
narrower than reality; widening the model once beats patching it per
symptom.

---

### 7. The meta-finding: the duplication is an output, not a habit

The 2026-08-02 audit named the recurring failure pattern: *knowledge
duplicated across hand-synchronized places with nothing enforcing
agreement*. This review's addition is causal: **that duplication is
generated, not chosen.** §1 generates it spatially — with no schema, every
new processor mints fresh hand-synced copies across 14 sites. §2 generates
it temporally — with no materialized facts, every phase re-derives the same
values and the copies must be kept in agreement by comments. The house
style then absorbs the structural pressure with the two tools it trusts,
comments and tests: `display.rs` was created to fix the `graph → cli`
inversion (which still exists, and `cli` re-exports the types straight
back); `phases.rs` is a 39-line cycle-breaker that re-enters its own cycle;
`build_context.rs` documents its own escape-hatch fields ("has no route to
the full Config"); `config/tests.rs` polices by hand what a schema would
make impossible. Each absorption is locally reasonable. Collectively the
module count grows while the topology stays tangled.

Also worth stating plainly: two of the project's rules — **no macros** and
**never create dummy instances** — are direct causes of §1 and §3
respectively. They may still be the right rules; but the trade should be
made with the price on the table, because right now the price is ~200 lines
of parallel hand lists, ~150 lines of policing tests, three match tables,
and a structurally second-class plugin system.

### Leverage ranking

1. **§1 — the processor schema split.** Removes the *generator* of the
   drift-bug class, unties the `config↔registries↔processors` knot, and is
   the prerequisite for any workspace split. Interim step available today
   at near-zero cost: the processor-level completeness test.
2. **§2, first slice — materialize `ResolvedInputs`; make `CacheKey`
   completeness un-forgeable.** Removes the generator of the
   cache-correctness class (the five-instant recompute, the eviction
   protocol, the deletion-by-stale-checksum edge). One written page —
   "which checksum keys a descriptor, and at which instant" — before any
   code.
3. **§4 — `Command` trait + injectable `Builder` + data/presenter split.**
   Not the deepest flaw, but the enabler: it is what makes §1 and §2 safe
   to execute, because it finally makes the command layer unit-testable
   (5 tests vs 31 entry points today).
4. **§3 — one home for config-dependent capabilities.** Contained; small
   since the Lua half of the section went with the Lua processor type.
5. **§5 — persistence `Store` trait + single-process decision + backend
   conformance suite.** Contained; mostly subtraction.
6. **§6 — widen the cache model** (per-file batch results, negative space
   in trees) — do after §2, which supplies the vocabulary it needs.
