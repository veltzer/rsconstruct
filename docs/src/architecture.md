# Architecture

This page describes RSB's internal design for contributors and those interested in how the tool works.

## Core concepts

### Processors

Processors implement the `ProductDiscovery` trait. Each processor:

1. **Auto-detects** whether it is relevant for the current project
2. Scans the project for source files matching its conventions
3. Creates **products** describing what to build
4. Executes the build for each product

Available processors: `tera`, `ruff`, `pylint`, `mypy`, `pyrefly`, `cc_single_file`, `cppcheck`, `clang_tidy`, `shellcheck`, `spellcheck`, `rumdl`, `sleep`, `make`, `cargo`, `yamllint`, `jq`, `jsonlint`, `taplo`, `json_schema`.

### Auto-detection

Every processor implements `auto_detect()`, which returns `true` if the processor appears relevant for the current project based on filesystem heuristics. This allows RSB to guess which processors a project needs without requiring manual configuration.

The `ProductDiscovery` trait requires four methods:

| Method | Purpose |
|---|---|
| `auto_detect(file_index)` | Return `true` if the project looks like it needs this processor |
| `discover(graph, file_index)` | Query the file index and add products to the build graph |
| `execute(product)` | Build a single product |
| `clean(product)` | Remove a product's outputs |

Both `auto_detect` and `discover` receive a `&FileIndex` — a pre-built index of all non-ignored files in the project (see [File indexing](#file-indexing) below).

Detection heuristics per processor:

| Processor | Detected when |
|---|---|
| `tera` | `templates/` directory contains files matching configured extensions |
| `ruff` | Project contains `.py` files (excluding `.venv/`, `__pycache__/`, etc.) |
| `pylint` | Same as `ruff` |
| `mypy` | Same as `ruff` |
| `pyrefly` | Same as `ruff` |
| `cc_single_file` | Configured source directory contains `.c` or `.cc` files |
| `cppcheck` | Same as `cc_single_file` |
| `clang_tidy` | Same as `cc_single_file` |
| `shellcheck` | Project contains `.sh` or `.bash` files |
| `spellcheck` | Project contains files matching configured extensions (e.g., `.md`) |
| `rumdl` | Project contains `.md` files |
| `sleep` | `sleep/` directory contains `.sleep` files |
| `make` | Project contains `Makefile` files |
| `cargo` | Project contains `Cargo.toml` files |
| `yamllint` | Project contains `.yml` or `.yaml` files |
| `jq` | Project contains `.json` files |
| `jsonlint` | Project contains `.json` files |
| `taplo` | Project contains `.toml` files |
| `json_schema` | Project contains `.json` files |

Run `rsb processors list` to see the auto-detection results for the current project.

### Products

A product represents a single build unit with:

- **Inputs** — source files that the product depends on
- **Outputs** — files that the product generates

### BuildGraph

The `BuildGraph` manages dependencies between products. It performs a topological sort to determine the correct build order, ensuring that dependencies are built before the products that depend on them.

### Executor

The executor runs products in dependency order. It supports:

- Sequential execution (default)
- Parallel execution of independent products (with `-j` flag)
- Dry-run mode (show what would be built)
- Keep-going mode (continue after errors)

## Interrupt handling

All external subprocess execution goes through `run_command()` in `src/processors/mod.rs`. Instead of calling `Command::output()` (which blocks until the process finishes), `run_command()` uses `Command::spawn()` followed by a poll loop:

1. Spawn the child process with piped stdout/stderr
2. Every 50ms, call `try_wait()` to check if the process has exited
3. Between polls, check the global `INTERRUPTED` flag (set by the Ctrl+C handler)
4. If interrupted, kill the child process immediately and return an error

This ensures that pressing Ctrl+C terminates running subprocesses within 50ms, even for long-running compilations or linter invocations. The sleep processor uses the same pattern — its sleep interval is broken into 50ms chunks with interrupt checks between them.

The global `INTERRUPTED` flag is an `AtomicBool` set once by the `ctrlc` handler in `main.rs` and checked by all threads.

## File indexing

RSB walks the project tree once at startup and builds a `FileIndex` — a sorted list of all non-ignored files. The walk is performed by the `ignore` crate (`ignore::WalkBuilder`), which natively handles:

- `.gitignore` — standard git ignore rules, including nested `.gitignore` files and negation patterns
- `.rsbignore` — project-specific ignore patterns using the same glob syntax as `.gitignore`

Processors never walk the filesystem themselves. Instead, `auto_detect` and `discover` receive a `&FileIndex` and query it with their scan configuration (extensions, exclude directories, exclude files). This replaces the previous design where each processor performed its own recursive walk.

## Build pipeline

1. **File indexing** — The project tree is walked once to build the `FileIndex`
2. **Discovery** — Each enabled processor queries the file index and creates products
3. **Graph construction** — Products are added to the `BuildGraph` with their dependencies
4. **Topological sort** — The graph is sorted to determine build order
5. **Cache check** — Each product's inputs are hashed (SHA-256) and compared against the cache
6. **Execution** — Stale products are rebuilt; up-to-date products are skipped or restored from cache
7. **Cache update** — Successfully built products have their outputs stored in the cache

## Determinism

Build order is deterministic:

- File discovery is sorted
- Processor iteration order is sorted
- Topological sort produces a stable ordering

This ensures that the same project always builds in the same order, regardless of filesystem ordering.

## Config-aware caching

Processor configuration (compiler flags, linter arguments, etc.) is hashed into cache keys. This means changing a config value like `cflags` will trigger rebuilds of affected products, even if the source files haven't changed.

## Cache storage

The cache lives in `.rsb/` and consists of:

- `db.redb` — redb database storing the object store index (maps product hashes to cached outputs)
- `objects/` — stored build artifacts (addressed by content hash)
- `deps.redb` — redb database storing source file dependencies (see [Dependency Caching](dependency-caching.md))

Cache restoration can use either hardlinks (fast, same filesystem) or copies (works across filesystems), configured via `restore_method`.

## Caching and clean behavior

The cache (`.rsb/`) stores build state to enable fast incremental builds:

- **Generators**: Cache stores copies of output files. After `rsb clean`, outputs are deleted but cache remains. Next `rsb build` restores outputs from cache (fast hardlink/copy) instead of regenerating.

- **Checkers**: No output files to cache. The cache entry itself serves as a "success marker". After `rsb clean` (nothing to delete), next `rsb build` sees the cache entry is valid and skips the check entirely (instant).

This ensures `rsb clean && rsb build` is fast for both types — generators restore from cache, checkers skip entirely.

## Subprocess execution

RSB uses two internal functions to run external commands:

- **`run_command()`** — by default captures stdout/stderr via OS pipes and only prints output on failure (quiet mode). Use `--show-output` flag to show all tool output. Use for compilers, linters, and any command where errors should be shown.

- **`run_command_capture()`** — always captures stdout/stderr via pipes. Use only when you need to parse the output (dependency analysis, version checks, Python config loading). Returns the output for processing.

### Parallel safety

When running with `-j`, each thread spawns its own subprocess. Each subprocess gets its own OS-level pipes for stdout/stderr, so there is no interleaving of output between concurrent tools. On failure, the captured output for that specific tool is printed atomically. This design requires no shared buffers or cross-thread output coordination.

## Path handling

**All paths are relative to project root.** RSB assumes it is run from the project root directory (where `rsb.toml` lives).

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
