# Architecture

This page describes RSB's internal design for contributors and those interested in how the tool works.

## Core concepts

### Processors

Processors implement the `ProductDiscovery` trait. Each processor:

1. **Auto-detects** whether it is relevant for the current project
2. Scans the project for source files matching its conventions
3. Creates **products** describing what to build
4. Executes the build for each product

Available processors: `template`, `ruff`, `pylint`, `cc_single_file`, `cpplint`, `spellcheck`, `sleep`.

### Auto-detection

Every processor implements `auto_detect()`, which returns `true` if the processor appears relevant for the current project based on filesystem heuristics. This allows RSB to guess which processors a project needs without requiring manual configuration.

The `ProductDiscovery` trait requires four methods:

| Method | Purpose |
|---|---|
| `auto_detect()` | Return `true` if the project looks like it needs this processor |
| `discover()` | Scan the project and add products to the build graph |
| `execute()` | Build a single product |
| `clean()` | Remove a product's outputs |

Detection heuristics per processor:

| Processor | Detected when |
|---|---|
| `template` | `templates/` directory contains files matching configured extensions |
| `ruff` | `pyproject.toml` exists, or `tests/` contains `.py` files |
| `pylint` | Same as `ruff` |
| `cc_single_file` | Configured source directory contains `.c` or `.cc` files |
| `cpplint` | Same as `cc_single_file` |
| `spellcheck` | Project contains files matching configured extensions (e.g., `.md`) |
| `sleep` | `sleep/` directory contains `.sleep` files |

Run `rsb processor auto` to see the auto-detection results for the current project.

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

## Build pipeline

1. **Discovery** — Each enabled processor scans for source files and creates products
2. **Graph construction** — Products are added to the `BuildGraph` with their dependencies
3. **Topological sort** — The graph is sorted to determine build order
4. **Cache check** — Each product's inputs are hashed (SHA-256) and compared against the cache
5. **Execution** — Stale products are rebuilt; up-to-date products are skipped or restored from cache
6. **Cache update** — Successfully built products have their outputs stored in the cache

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

- `index.json` — maps product hashes to cached outputs
- `objects/` — stored build artifacts (addressed by content hash)
- `deps/` — dependency files (e.g., gcc `-MMD` output for header tracking)

Cache restoration can use either hardlinks (fast, same filesystem) or copies (works across filesystems), configured via `restore_method`.
