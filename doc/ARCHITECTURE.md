# RSConstruct: orientation map

Written for an AI agent starting work in this repository. It says where
things are, how a build flows through the code, and which rules bite. It is a
map, not a reference: each section ends with the file or chapter that holds
the detail. Verified against the source at 0.9.167 (2026-11-16).

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

Unix only (Linux, macOS). The repo builds itself: CI runs `rsconstruct build`
against this repo's own `rsconstruct.toml`.

## Other documents, and which to trust

| Path | What it is |
|---|---|
| `CLAUDE.md` | The binding rules. Read first; it overrides everything here. |
| `docs/src/` | The mdBook: user docs, plus `docs/src/internal/` for maintainers. |
| `docs/src/internal/README.md` | Reading order and one-line summary of each internal chapter. |
| `docs/src/internal/architecture.md` | The maintainers' architecture chapter. Parts lag the code (see below). |
| `docs/src/internal/processor-contract.md` | What every processor must uphold. |
| `docs/src/internal/coding-standards.md` | Conventions enforced in review. |
| `docs/src/internal/rejected-problems.md` | Findings deliberately rejected. Check before "fixing" something odd. |
| `ARCHITECTURE.md` (repo root) | A 2026-08 architectural *assessment*: six structural deficiencies with STATUS notes. History and rationale, not a map. |
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
|---|---|
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
   `Config::load` merges user config, then `rsconstruct.toml`, then
   `rsconstruct.local.toml`, after per-file `[vars]` substitution; validates;
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
|---|---|---|
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
|---|---|
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
  pedantic and nursery on. The crate-level allow list in `main.rs` is
  closed; a new allow goes at the call site with a reason.
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
- **Edit Rust by hand.** No sed, awk or scripts over `src/`.
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
|---|---|
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
| Why something is the way it is | `docs/src/internal/rejected-problems.md`, `architecture-observations.md`, root `ARCHITECTURE.md` |
