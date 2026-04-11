# Architecture

This page describes RSConstruct's internal design for contributors and those interested in how the tool works.

## Core concepts

### Processors

Processors implement the `ProductDiscovery` trait. Each processor:

1. **Auto-detects** whether it is relevant for the current project
2. Scans the project for source files matching its conventions
3. Creates **products** describing what to build
4. Executes the build for each product

Run `rsconstruct processors list` to see all available processors and their auto-detection results.

### Auto-detection

Every processor implements `auto_detect()`, which returns `true` if the processor appears relevant for the current project based on filesystem heuristics. This allows RSConstruct to guess which processors a project needs without requiring manual configuration.

The `ProductDiscovery` trait requires four methods:

| Method | Purpose |
|---|---|
| `auto_detect(file_index)` | Return `true` if the project looks like it needs this processor |
| `discover(graph, file_index)` | Query the file index and add products to the build graph |
| `execute(product)` | Build a single product |
| `clean(product)` | Remove a product's outputs |

Both `auto_detect` and `discover` receive a `&FileIndex` — a pre-built index of all non-ignored files in the project (see [File indexing](#file-indexing) below).

Detection heuristics per processor:

| Processor | Type | Detected when |
|---|---|---|
| `tera` | Generator | `templates/` directory contains files matching configured extensions |
| `ruff` | Checker | Project contains `.py` files |
| `pylint` | Checker | Project contains `.py` files |
| `mypy` | Checker | Project contains `.py` files |
| `pyrefly` | Checker | Project contains `.py` files |
| `cc_single_file` | Generator | Configured source directory contains `.c` or `.cc` files |
| `cppcheck` | Checker | Configured source directory contains `.c` or `.cc` files |
| `clang_tidy` | Checker | Configured source directory contains `.c` or `.cc` files |
| `shellcheck` | Checker | Project contains `.sh` or `.bash` files |
| `zspell` | Checker | Project contains files matching configured extensions (e.g., `.md`) |
| `aspell` | Checker | Project contains `.md` files |
| `ascii` | Checker | Project contains `.md` files |
| `rumdl` | Checker | Project contains `.md` files |
| `mdl` | Checker | Project contains `.md` files |
| `markdownlint` | Checker | Project contains `.md` files |
| `make` | Checker | Project contains `Makefile` files |
| `cargo` | Mass Generator | Project contains `Cargo.toml` files |
| `sphinx` | Mass Generator | Project contains `conf.py` files |
| `mdbook` | Mass Generator | Project contains `book.toml` files |
| `yamllint` | Checker | Project contains `.yml` or `.yaml` files |
| `jq` | Checker | Project contains `.json` files |
| `jsonlint` | Checker | Project contains `.json` files |
| `json_schema` | Checker | Project contains `.json` files |
| `taplo` | Checker | Project contains `.toml` files |
| `pip` | Mass Generator | Project contains `requirements.txt` files |
| `npm` | Mass Generator | Project contains `package.json` files |
| `gem` | Mass Generator | Project contains `Gemfile` files |
| `pandoc` | Generator | Project contains `.md` files |
| `markdown2html` | Generator | Project contains `.md` files |
| `marp` | Generator | Project contains `.md` files |
| `mermaid` | Generator | Project contains `.mmd` files |
| `drawio` | Generator | Project contains `.drawio` files |
| `a2x` | Generator | Project contains `.txt` (AsciiDoc) files |
| `pdflatex` | Generator | Project contains `.tex` files |
| `libreoffice` | Generator | Project contains `.odp` files |
| `pdfunite` | Generator | Source directory contains subdirectories with PDF-source files |
| `iyamlschema` | Checker | Project contains `.yml` or `.yaml` files |
| `yaml2json` | Generator | Project contains `.yml` or `.yaml` files |
| `imarkdown2html` | Generator | Project contains `.md` files |
| `tags` | Generator | Project contains `.md` files with YAML frontmatter |

Run `rsconstruct processors list` to see the auto-detection results for the current project.

### Products

A product represents a single build unit with:

- **Inputs** — source files that the product depends on
- **Outputs** — files that the product generates
- **Output directory** (optional) — for creators, the directory whose entire contents are cached and restored as a unit

### BuildGraph

The `BuildGraph` manages dependencies between products. It performs a topological sort to determine the correct build order, ensuring that dependencies are built before the products that depend on them.

### Executor

The executor runs products in dependency order. It supports:

- Sequential execution (default)
- Parallel execution of independent products (with `-j` flag)
- Dry-run mode (show what would be built)
- Keep-going mode (continue after errors)
- Batch execution (group multiple products into one tool invocation)

### Incremental rebuild after partial failure

Each product is cached independently after successful execution. If a build is
interrupted or fails partway through, the next run only rebuilds products that
don't have valid cache entries:

- **Non-batch mode** (default fail-fast, `chunk_size=1`): Each product executes
  and is cached individually. If the build stops after 400 of 800 products, the
  next run skips the 400 cached successes and rebuilds the remaining 400.

- **Batch mode with external tools** (`--keep-going` or explicit `--batch-size`):
  The external tool receives all files in the batch in one invocation. If the tool
  exits with an error, all products in that batch are marked failed — there is no
  way to determine which outputs are valid from a single exit code. On the next
  run, all products from the failed batch are rebuilt.

- **Batch mode with internal processors** (e.g., `imarkdown2html`, `isass`, `ipdfunite`):
  These process files sequentially in-process and return per-file results, so
  partial failure is handled correctly even in batch mode — only the failed
  products are rebuilt.

## Interrupt handling

All external subprocess execution goes through `run_command()` in `src/processors/mod.rs`. Instead of calling `Command::output()` (which blocks until the process finishes), `run_command()` uses `Command::spawn()` followed by a poll loop:

1. Spawn the child process with piped stdout/stderr
2. Every 50ms, call `try_wait()` to check if the process has exited
3. Between polls, check the global `INTERRUPTED` flag (set by the Ctrl+C handler)
4. If interrupted, kill the child process immediately and return an error

This ensures that pressing Ctrl+C terminates running subprocesses within 50ms, even for long-running compilations or linter invocations.

The global `INTERRUPTED` flag is an `AtomicBool` set once by the `ctrlc` handler in `main.rs` and checked by all threads.

## File indexing

RSConstruct walks the project tree once at startup and builds a `FileIndex` — a sorted list of all non-ignored files. The walk is performed by the `ignore` crate (`ignore::WalkBuilder`), which natively handles:

- `.gitignore` — standard git ignore rules, including nested `.gitignore` files and negation patterns
- `.rsconstructignore` — project-specific ignore patterns using the same glob syntax as `.gitignore`

Processors never walk the filesystem themselves. Instead, `auto_detect` and `discover` receive a `&FileIndex` and query it with their scan configuration (src_extensions, exclude directories, exclude files). This replaces the previous design where each processor performed its own recursive walk.

## Build pipeline

This is the core algorithm — every `rsconstruct build` follows these phases
in order. Use `--phases` to see timing for each phase.

### Phase 1: File indexing

The project tree is walked once to build the `FileIndex` — a sorted list of
all non-ignored files. This is the only filesystem walk; all subsequent file
lookups go through the index. See [File indexing](#file-indexing) below.

### Phase 2: Discovery (fixed-point loop)

Each enabled processor queries the file index and adds products to the
`BuildGraph`. Discovery runs in a **fixed-point loop** to handle
cross-processor dependencies:

```
file_index = walk filesystem
loop (max 10 passes):
    for each processor:
        processor.discover(graph, file_index)
    if no new products were added → break
    collect outputs from new products
    inject them as virtual files into file_index
```

On each pass, processors may re-declare existing products (silently
deduplicated) or discover new products whose inputs are virtual files from
upstream generators. The loop converges when a full pass adds nothing new.
Most projects converge in 1 pass; projects with generator → checker/generator
chains converge in 2.

See [Cross-Processor Dependencies](cross-processor-dependencies.md) for
details on deduplication and the virtual file mechanism.

### Phase 3: Dependency analysis

Dependency analyzers (e.g., the C/C++ header scanner) run against the graph
to add additional input edges. For example, if `main.c` includes `util.h`,
the analyzer adds `util.h` as an input to the `main.c` product. Results are
cached in `deps.redb` for incremental builds.

### Phase 4: Tool version hashing

For each processor with a tool lock entry (`rsconstruct tools lock`), the
locked tool version hash is appended to the product's config hash. This
ensures that upgrading a tool (e.g., `ruff` 0.4 → 0.5) triggers rebuilds
even if source files haven't changed.

### Phase 5: Dependency resolution

`resolve_dependencies()` scans the graph for products whose inputs match
other products' outputs. When found, it creates a dependency edge — the
producer must complete before the consumer can start. This is how
cross-processor ordering works automatically (e.g., pandoc runs before the
explicit site generator because pandoc's HTML outputs are the site
generator's inputs).

After resolution, the graph is topologically sorted to produce the execution
order.

### Phase 6: Classify

Each product is classified as one of:

- **Skip (up-to-date)** — input checksum matches the cache entry and all
  outputs exist on disk. No work needed.
- **Restore** — input checksum matches a cache entry but outputs are missing
  (e.g., after `rsconstruct clean`). Outputs are restored from cache via
  hardlink or copy.
- **Build (stale)** — input checksum doesn't match any cache entry. The
  product must be rebuilt.

Input checksums are computed by hashing all input files (SHA-256). The mtime
pre-check (`mtime_check = true`, default) skips rehashing files whose mtime
hasn't changed since the last build.

### Phase 7: Execute

Products are executed in topological order, respecting dependency edges.
Independent products at the same dependency level run in parallel (controlled
by `-j` / `RSCONSTRUCT_THREADS`). Batch-capable processors group their
products into a single tool invocation.

**Batch chunk sizing:** In fail-fast mode (default), batch chunk size is 1 —
each product executes independently even for batch-capable processors. With
`--keep-going`, all products are sent in one chunk. With `--batch-size N`,
chunks are limited to N products. This means fail-fast mode gives the best
incremental recovery after partial failure.

For each product:
1. Compute input checksum (if not already done in classify)
2. Check cache — skip or restore if possible
3. Execute the processor's command
4. On success: store outputs in the cache (content-addressed under
   `.rsconstruct/objects/`)
5. On failure: report error (or continue if `--keep-going`)

## Determinism

Build order is deterministic:

- File discovery is sorted
- Processor iteration order is sorted
- Topological sort produces a stable ordering

This ensures that the same project always builds in the same order, regardless of filesystem ordering.

## Caching

See [Cache System](cache.md) for full details on cache keys, storage format, rebuild classification, and per-processor caching behavior.

## Subprocess execution

RSConstruct uses two internal functions to run external commands:

- **`run_command()`** — by default captures stdout/stderr via OS pipes and only prints output on failure (quiet mode). Use `--show-output` flag to show all tool output. Use for compilers, linters, and any command where errors should be shown.

- **`run_command_capture()`** — always captures stdout/stderr via pipes. Use only when you need to parse the output (dependency analysis, version checks, Python config loading). Returns the output for processing.

### Parallel safety

When running with `-j`, each thread spawns its own subprocess. Each subprocess gets its own OS-level pipes for stdout/stderr, so there is no interleaving of output between concurrent tools. On failure, the captured output for that specific tool is printed atomically. This design requires no shared buffers or cross-thread output coordination.

## Path handling

**All paths are relative to project root.** RSConstruct assumes it is run from the project root directory (where `rsconstruct.toml` lives).

### Internal paths (always relative)
- `Product.inputs` and `Product.outputs` — stored as relative paths
- `FileIndex` — returns relative paths from `scan()` and `query()`
- Cache keys (`Product.cache_key()`) — use relative paths, enabling cache sharing across different checkout locations
- Cache entries (`CacheEntry.outputs[].path`) — stored as relative paths

### Processor execution
- Processors pass relative paths directly to external tools
- Processors set `cmd.current_dir(project_root)` to ensure tools resolve paths correctly
- `fs::read()`, `fs::write()`, etc. work directly with relative paths since cwd is project root

### Exception: Processors requiring absolute paths
If a processor absolutely must use absolute paths (e.g., for a tool that doesn't respect current directory), it should:
1. Store the `project_root` in the processor struct
2. Join paths with `project_root` only at execution time
3. Never store absolute paths in `Product.inputs` or `Product.outputs`

### Why relative paths?
- **Cache portability** — cache keys don't include machine-specific absolute paths
- **Remote cache sharing** — same project checked out to different paths can share cache
- **Simpler code** — no need to strip prefixes for display or storage
