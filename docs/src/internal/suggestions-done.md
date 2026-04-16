# Completed Suggestions

Items from `suggestions.md` that have been implemented.

## Completed Features

- **Remote caching** — See [Remote Caching](../remote-caching.md). Share build artifacts across machines via S3, HTTP, or filesystem.
- **Lua plugin system** — See [Lua Plugins](../plugins.md). Define custom processors in Lua without forking rsconstruct.
- **Tool version locking** — `rsconstruct tools lock` locks and verifies external tool versions. Tool versions are included in cache keys.
- **JSON output mode** — `--json` flag for machine-readable JSON Lines output (build_start, product_start, product_complete, build_summary events).
- **Native C/C++ include scanner** — Default `include_scanner = "native"` uses regex-based scanning. Falls back to `include_scanner = "compiler"` (gcc -MM).
- **`--processors` flag** — `rsconstruct build -p tera,ruff` and `rsconstruct watch -p tera` filter which processors run.
- **Colored diff on config changes** — When processor config changes trigger rebuilds, rsconstruct shows what changed with colored diff output.
- **Batch processing** — ruff, pylint, shellcheck, zspell, mypy, and rumdl all support batch execution via `execute_batch()`.
- **Progress bar** — Uses `indicatif` crate. Progress bar sized to actual work (excludes instant skips), hidden in verbose/JSON mode.
- **Emit `ProductStart` JSON events** — Emitted before each product starts executing, pairs with `ProductComplete` for per-product timing.
- **mypy processor** — Python type checking with mypy. Batch-capable. Auto-detects `mypy.ini` as extra input.
- **Explain commands** — `--explain` flag shows skip/restore/rebuild reasons for each product during build.

## Completed Code Consolidation

- **Collapsed `checker_config!` macro variants** — Merged `@basic`, `@with_auto_inputs`, and `@with_linter` into two internal variants (`@no_linter` and `@with_linter`).
- **Added `batch` field to all manually-defined processor configs** — All processor configs now support `batch = false` to disable batching per-project.
- **Replaced trivial checker files with `simple_checker!` macro** — 25 trivial checkers reduced from ~35 lines each to 3-5 lines (~800 lines eliminated).
- **Unified `lint_files`/`check_files` naming** — All checkers now use `check_files` consistently.
- **Moved `should_process` guard into macro** — Added `guard: scan_root` built-in to `impl_checker!`, removed boilerplate `should_process()` from 7 processors.
- **Simplified `KnownFields`** — Scan config fields auto-appended by validation layer via `SCAN_CONFIG_FIELDS` constant; `KnownFields` impls only list their own fields.
- **Extracted `WordManager` for spellcheck/aspell** — Shared word-file management (loading, collecting, flushing, execute/batch patterns) in `word_manager.rs`.

## Completed New Processors

- **mypy** — Python type checking using `mypy`. Batch-capable. Config: `checker`, `args`, `dep_inputs`, `scan`.
- **yamllint** — Lint YAML files using `yamllint`. `src/processors/checkers/yamllint.rs`.
- **jsonlint** — Validate JSON files for syntax errors. `src/processors/checkers/jsonlint.rs`.
- **taplo (toml-lint)** — Validate TOML files using `taplo`. `src/processors/checkers/taplo.rs`.
- **markdownlint** — Lint Markdown files for structural issues. Uses `mdl` or `markdownlint-cli`.
- **pandoc** — Convert Markdown to other formats (PDF, HTML, EPUB). Generator processor.
- **jinja2** — Render Jinja2 templates (`.j2`) via Python jinja2 library. `src/processors/generators/jinja2.rs`.
- **black** — Python formatting verification using `black --check`. `src/processors/checkers/black.rs`.
- **rust_single_file** — Compile single-file Rust programs to executables. `src/processors/generators/rust_single_file.rs`.
- **sass** — Compile SCSS/SASS files to CSS. `src/processors/generators/sass.rs`.
- **protobuf** — Compile `.proto` files to generated code using `protoc`. `src/processors/generators/protobuf.rs`.
- **pytest** — Run Python test files with pytest. `src/processors/checkers/pytest.rs`.
- **doctest** — Run Python doctests via `python3 -m doctest`. `src/processors/checkers/doctest.rs`.

## Completed Test Coverage

- **Ruff/pylint processor tests** — `tests/processors/ruff.rs` and `tests/processors/pylint.rs` with integration tests.
- **Make processor tests** — `tests/processors/make.rs` with Makefile discovery and execution tests.
- **All generator processor tests** — Integration tests for all 14 previously untested generators: a2x, drawio, gem, libreoffice, markdown, marp, mermaid, npm, pandoc, pdflatex, pdfunite, pip, sphinx.
- **All checker processor tests** — Integration tests for all 5 previously untested checkers: ascii, aspell, markdownlint, mdbook, mdl.

## Completed Caching & Performance

- **Lazy file hashing (mtime-based)** — `mtime_check` config (default `true`), `fast_checksum()` with MTIME_TABLE. Stores `(path, mtime, checksum)` tuples. Disable with `--no-mtime`.
- **Compressed cache objects** — Optional zstd compression for `.rsconstruct/objects/`. Config: `compression = true` in `[cache]`. Incompatible with hardlink restore (must use `restore_method = "copy"`). Checksums computed on original content for stable cache keys.

## Completed Developer Experience

- **`--quiet` flag** — `-q`/`--quiet` suppresses all output except errors. Useful for CI scripts that only care about exit code.
- **Flaky product detection / retry** — `--retry=N` retries failed products up to N times. Reports FLAKY (passed on retry) vs FAILED status in build summary.
- **Actionable error messages** — `rsconstruct tools check` shows install hints for missing tools (e.g., "install with: pip install ruff").
- **Build profiling / tracing** — `--trace=file.json` generates Chrome trace format output viewable in `chrome://tracing` or Perfetto UI.
- **`rsconstruct build <target>`** — Build specific targets by name or pattern via `--target` glob patterns and `-d/--dir` flags.
- **`rsconstruct why <file>` / Explain rebuilds** — `--explain` flag shows why each product is skipped, restored, or rebuilt.
- **`rsconstruct doctor`** — Diagnose build environment: checks config, tools, and versions. Full implementation in `src/builder/doctor.rs`.
- **`rsconstruct sloc`** — Source lines of code statistics with COCOMO effort/cost estimation. `src/builder/sloc.rs`.

## Completed Quick Wins

- **Batch processing for more processors** — All checker processors that support multiple file arguments now use batching.
- **Progress bar for long builds** — Implemented with `indicatif`, shows `[elapsed] [bar] pos/len message`.
- **`--processors` flag for build and watch** — Filter processors with `-p` flag.
- **Emit `ProductStart` JSON events** — Wired up and emitted before execution.
- **Colored diff on config changes** — Shows colored JSON diff when processor config changes.

## Completed Features (v0.3.7)

- **`RSCONSTRUCT_THREADS` env var** — Set parallelism via environment variable instead of `-j`. Priority: CLI `-j` > `RSCONSTRUCT_THREADS` > config `parallel`.
- **Global `output_dir` in `[build]`** — Global output directory prefix (default: `"out"`). Processor defaults like `out/marp` are remapped when the global is changed (e.g., `output_dir = "build"` makes marp output to `build/marp`). Individual processors can still override their `output_dir` explicitly.
- **Named processor instance output directories** — When multiple instances of the same processor are declared (e.g., `[processor.marp.slides]` and `[processor.marp.docs]`), each instance defaults to `out/{instance_name}` (e.g., `out/marp.slides`, `out/marp.docs`) instead of sharing the same output directory.
- **Named processor instance names in error reporting** — When multiple instances of the same processor exist, error messages, build progress, and statistics use the full instance name (e.g., `[pylint.core]`, `[pylint.tests]`). Single instances continue to use just the processor type name.
- **`processors config` without config file** — `rsconstruct processors config <name>` now works without an `rsconstruct.toml`, showing the default configuration (same as `defconfig`).
- **`tags collect` command** — `rsconstruct tags collect` scans the tags database for tags that are not in the tag collection (`tags_dir`) and adds them to the appropriate `.txt` files. Key:value tags go to `{key}.txt`, bare tags go to `tags.txt`.
- **`rsconstruct status` shows 0-file processors** — Processors declared in the config that match no files are now shown in `status` output and the `--breakdown` summary, making it easy to spot misconfigured or unnecessary processors.
- **`smart remove-no-file-processors`** — New command `rsconstruct smart remove-no-file-processors` removes `[processor.*]` sections from `rsconstruct.toml` for processors that don't match any files. Handles both single and named instances.
- **`cc_single_file` output_dir from config** — The `cc_single_file` processor now reads its output directory from the config `output_dir` field instead of hardcoding `out/cc_single_file`. This fixes named instances (e.g., `cc_single_file.gcc` and `cc_single_file.clang`) which previously collided on the same output directory.
- **`clean unknown` respects .gitignore** — `rsconstruct clean unknown` now skips gitignored files. Previously it disabled .gitignore handling, causing intentionally ignored files (IDE configs, virtualenvs, `*.pyc`, etc.) to be flagged as unknown. RSConstruct outputs are still correctly identified via the build graph, so nothing is missed. Use `--no-gitignore` to include gitignored files.
- **Cross-processor dependencies (fixed-point discovery)** — Generator outputs are now visible to downstream processors on the first build. Discovery runs in a fixed-point loop: after each pass, declared outputs are injected as virtual files into the FileIndex, and discovery re-runs until no new products are found. This means a generator that creates `.md` files can feed pandoc/tags/spell-checkers in a single build, without needing a second build.

## Completed Architecture Refactors

- **Config provenance tracking** — Every config field now carries `FieldProvenance` (UserToml with line number, ProcessorDefault, ScanDefault, OutputDirDefault, SerdeDefault). `rsconstruct config show` annotates every field with its source. Uses `toml_edit::Document` for span capture.
- **`BuildContext` replacing process globals** — The three process globals (`INTERRUPTED`, `RUNTIME`, `INTERRUPT_SENDER`) are replaced by a `BuildContext` struct threaded through the `Processor` trait's `execute()`/`execute_batch()`, the executor, analyzers, and remote cache. Signal handler uses `Arc<BuildContext>`.
- **`BuildPolicy` trait** — Extracted from the executor. `classify_products` delegates per-product skip/restore/rebuild decisions to a `&dyn BuildPolicy`. `IncrementalPolicy` implements the current logic. Future policies (dry-run, always-rebuild, time-windowed) are a single trait impl.
- **`ObjectStore` decomposition** — `mod.rs` split from 664 → 223 lines into focused submodules: `blobs.rs` (content-addressed storage), `descriptors.rs` (cache descriptor CRUD), `restore.rs` (restore/needs_rebuild/can_restore/explain).

## Completed Features (latest)

- **`rsconstruct status --json`** — JSON output with per-processor counts (`up_to_date`, `restorable`, `stale`, `new`, `total`, `native`) and totals. Activated by `--json` flag.
- **Selective processor cleaning** — `rsconstruct clean outputs -p ruff,pylint` cleans only those processors' outputs. Without `-p`, cleans everything.
- **Prettier processor** — Checker using `prettier --check`. Batch-capable. Scans `.js/.jsx/.ts/.tsx/.mjs/.cjs/.css/.scss/.less/.html/.json/.md/.yaml/.yml`. `src/processors/checkers/prettier.rs`.
- **Bare `clean` requires subcommand** — `rsconstruct clean` now errors with usage hint instead of silently defaulting to `clean outputs`.
- **Nondeterministic test race fix** — Fixed TOCTOU race in `store_descriptor` where parallel writers could get `Permission denied`. Now retries after forcing writable on first failure.
- **Suppress status line for non-build commands** — The `Exited with SUCCESS/ERROR` footer only shows for `build`, `watch`, and `clean`.
