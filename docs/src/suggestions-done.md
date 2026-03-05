# Completed Suggestions

Items from `suggestions.md` that have been implemented.

## Completed Features

- **Remote caching** — See [Remote Caching](remote-caching.md). Share build artifacts across machines via S3, HTTP, or filesystem.
- **Lua plugin system** — See [Lua Plugins](plugins.md). Define custom processors in Lua without forking rsbuild.
- **Tool version locking** — `rsbuild tools lock` locks and verifies external tool versions. Tool versions are included in cache keys.
- **JSON output mode** — `--json` flag for machine-readable JSON Lines output (build_start, product_start, product_complete, build_summary events).
- **Native C/C++ include scanner** — Default `include_scanner = "native"` uses regex-based scanning. Falls back to `include_scanner = "compiler"` (gcc -MM).
- **`--processors` flag** — `rsbuild build -p tera,ruff` and `rsbuild watch -p tera` filter which processors run.
- **Colored diff on config changes** — When processor config changes trigger rebuilds, rsbuild shows what changed with colored diff output.
- **Batch processing** — ruff, pylint, shellcheck, spellcheck, mypy, and rumdl all support batch execution via `execute_batch()`.
- **Progress bar** — Uses `indicatif` crate. Progress bar sized to actual work (excludes instant skips), hidden in verbose/JSON mode.
- **Emit `ProductStart` JSON events** — Emitted before each product starts executing, pairs with `ProductComplete` for per-product timing.
- **mypy processor** — Python type checking with mypy. Batch-capable. Auto-detects `mypy.ini` as extra input.
- **Explain commands** — `--explain` flag shows skip/restore/rebuild reasons for each product during build.

## Completed New Processors

- **mypy** — Python type checking using `mypy`. Batch-capable. Config: `checker`, `args`, `extra_inputs`, `scan`.

## Completed Test Coverage

- **Ruff/pylint processor tests** — `tests/processors/ruff.rs` and `tests/processors/pylint.rs` with integration tests.
- **Make processor tests** — `tests/processors/make.rs` with Makefile discovery and execution tests.

## Completed Caching & Performance

- **Lazy file hashing (mtime-based)** — `mtime_check` config (default `true`), `fast_checksum()` with MTIME_TABLE. Stores `(path, mtime, checksum)` tuples. Disable with `--no-mtime`.
- **Compressed cache objects** — Optional zstd compression for `.rsbuild/objects/`. Config: `compression = true` in `[cache]`. Incompatible with hardlink restore (must use `restore_method = "copy"`). Checksums computed on original content for stable cache keys.

## Completed Developer Experience

- **`--quiet` flag** — `-q`/`--quiet` suppresses all output except errors. Useful for CI scripts that only care about exit code.
- **Flaky product detection / retry** — `--retry=N` retries failed products up to N times. Reports FLAKY (passed on retry) vs FAILED status in build summary.
- **Actionable error messages** — `rsbuild tools check` shows install hints for missing tools (e.g., "install with: pip install ruff").

## Completed Quick Wins

- **Batch processing for more processors** — All checker processors that support multiple file arguments now use batching.
- **Progress bar for long builds** — Implemented with `indicatif`, shows `[elapsed] [bar] pos/len message`.
- **`rsbuild why <file>` / Explain rebuilds** — `--explain` flag shows why each product is skipped, restored, or rebuilt.
- **`--processors` flag for build and watch** — Filter processors with `-p` flag.
- **Emit `ProductStart` JSON events** — Wired up and emitted before execution.
- **Colored diff on config changes** — Shows colored JSON diff when processor config changes.
