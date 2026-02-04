# Suggestions

Ideas for future improvements, organized by category.

## Completed Features

Features that have been implemented and are documented elsewhere:

- **Remote caching** — See [Remote Caching](remote-caching.md). Share build artifacts across machines via S3, HTTP, or filesystem.
- **Lua plugin system** — See [Lua Plugins](plugins.md). Define custom processors in Lua without forking rsb.
- **Tool version locking** — See `rsb tools lock`. Lock and verify external tool versions for reproducibility.
- **JSON output mode** — Use `--json` flag for machine-readable JSON Lines output.

## Missing Test Coverage

### No ruff/pylint processor tests
- `tests/processors/` has tests for cc, sleep, spellcheck, and template, but not for ruff or pylint.
- Add integration tests for both Python linting processors.

### No make processor tests
- `tests/processors/` has no tests for the make processor.
- Add integration tests covering Makefile discovery and execution.

## New Processors

### Linting / Checking (stub-based)

#### yamllint
- Lint YAML files (`.yml`, `.yaml`) using `yamllint`.
- Catches syntax errors and style violations.
- Config: `linter` (default `"yamllint"`), `args`, `extra_inputs`, `scan`.

#### jsonlint
- Validate JSON files (`.json`) for syntax errors.
- Could use `python3 -m json.tool` or a dedicated tool like `jsonlint`.
- Config: `linter`, `args`, `extra_inputs`, `scan`.

#### toml-lint
- Validate TOML files (`.toml`) for syntax errors.
- Could use `taplo check` or a built-in Rust parser.
- Config: `linter` (default `"taplo"`), `args`, `extra_inputs`, `scan`.

#### markdownlint
- Lint Markdown files (`.md`) for structural issues (complements spellcheck which only checks spelling).
- Uses `mdl` or `markdownlint-cli`.
- Config: `linter` (default `"mdl"`), `args`, `extra_inputs`, `scan`.

#### mypy
- Python type checking using `mypy`.
- Batch-capable like ruff/pylint.
- Config: `args`, `extra_inputs`, `scan`.

#### black-check
- Python formatting verification using `black --check`.
- Verifies files are formatted without modifying them.
- Config: `args`, `extra_inputs`, `scan`.

### Compilation / Generation

#### rust_single_file
- Compile single-file Rust programs (`.rs`) to executables, like cc_single_file but for Rust.
- Useful for exercise/example repositories.
- Config: `rustc` (default `"rustc"`), `flags`, `output_suffix`, `extra_inputs`, `scan`.

#### sass
- Compile `.scss`/`.sass` files to `.css`.
- Single-file transformation using `sass` or `dart-sass`.
- Config: `compiler` (default `"sass"`), `args`, `extra_inputs`, `scan`.

#### protobuf
- Compile `.proto` files to generated code using `protoc`.
- Config: `protoc` (default `"protoc"`), `args`, `language` (default `"cpp"`), `extra_inputs`, `scan`.

#### pandoc
- Convert Markdown (`.md`) to other formats (PDF, HTML, EPUB) using `pandoc`.
- Single-file transformation.
- Config: `output_format` (default `"html"`), `args`, `extra_inputs`, `scan`.

### Testing

#### pytest
- Run Python test files and produce pass/fail stubs.
- Each `test_*.py` file becomes a product.
- Config: `runner` (default `"pytest"`), `args`, `extra_inputs`, `scan` (default extensions `["test_*.py"]`).

#### doctest
- Run Python doctests and produce stubs.
- Each `.py` file with doctests produces a stub.
- Config: `args`, `extra_inputs`, `scan`.

## Build Execution

### ~~Remote caching~~ *(Done)*
- See [Remote Caching](remote-caching.md).

### Distributed builds
- Run builds across multiple machines, similar to distcc or icecream for C/C++.
- A coordinator node distributes work to worker nodes, each running rsb in worker mode.
- Workers execute products and return outputs to the coordinator, which caches them locally.
- Complements remote caching: remote cache avoids rebuilding, distributed builds speed up unavoidable rebuilds.
- Configuration could be:
  ```toml
  [build]
  workers = ["worker1.local:9000", "worker2.local:9000"]
  ```
- Challenges include: network overhead for small products, ensuring identical tool versions across workers, and handling products that require local filesystem access.
- Bazel's remote execution and Pants's remote execution both solve this problem.

### Sandboxed execution
- Run each processor in an isolated environment where it can only access its declared inputs.
- Prevents accidental undeclared dependencies (e.g., a linter reading a file that isn't listed as an input).
- Bazel and Buck2 both enforce this. On Linux, namespaces can provide lightweight sandboxing without container overhead.

### Content-addressable outputs (unchanged output pruning)
- Currently rsb hashes inputs to detect staleness. Hashing outputs too would allow skipping downstream rebuilds when an input changes but produces identical output (e.g., reformatting a comment in a C file that doesn't change the compiled binary).
- Bazel calls this "unchanged output pruning."

### Persistent daemon mode
- Keep rsb running as a background daemon to avoid startup overhead.
- Benefits:
  - **Instant file index**: File tree is kept in memory and updated via inotify/FSEvents
  - **Warm Lua VMs**: Lua plugin interpreters stay loaded
  - **Connection pooling**: Remote cache connections stay open
  - **Faster incremental builds**: No process startup, no config parsing, no dependency graph reconstruction
- Usage:
  ```bash
  rsb daemon start          # Start the daemon
  rsb build                 # Connects to daemon automatically
  rsb daemon stop           # Stop the daemon
  rsb daemon status         # Check if daemon is running
  ```
- The daemon listens on a Unix socket (`.rsb/daemon.sock`) or TCP port.
- Client commands (`rsb build`, `rsb status`, etc.) detect the daemon and delegate to it.
- File watching is built into the daemon — `rsb watch` becomes just a client that triggers rebuilds on file events.
- Daemon auto-exits after idle timeout (configurable).
- Similar to: Watchman (Facebook), Buck2 daemon, Gradle daemon.
- Configuration:
  ```toml
  [daemon]
  enabled = true
  idle_timeout = "10m"
  socket = ".rsb/daemon.sock"
  ```

### Build profiles
- Named configuration sets for different build scenarios.
- Define profiles in `rsb.toml`:
  ```toml
  [profile.ci]
  parallel = 0                    # Use all cores
  cache.remote = "s3://ci-cache"
  cache.remote_push = true
  processor.enabled = ["ruff", "pylint", "mypy", "pytest"]

  [profile.dev]
  parallel = 4
  cache.remote_push = false       # Don't pollute CI cache
  processor.enabled = ["ruff"]    # Fast feedback, skip slow linters

  [profile.release]
  processor.cc_single_file.cflags = ["-O3", "-DNDEBUG"]
  processor.cc_single_file.ldflags = ["-s"]
  ```
- Usage:
  ```bash
  rsb build --profile=ci
  rsb build --profile=dev
  RSB_PROFILE=ci rsb build       # Environment variable
  ```
- Profiles inherit from the base configuration and override specific values.
- Default profile can be set:
  ```toml
  [build]
  default_profile = "dev"
  ```
- Use cases:
  - CI vs local development settings
  - Debug vs release builds
  - Different linter strictness levels
  - Platform-specific configurations

### Conditional processors
- Enable or disable processors based on conditions.
- Conditions can check: environment variables, file existence, git branch, or custom commands.
- Configuration:
  ```toml
  [processor.mypy]
  enabled_if.env = "CI"           # Only in CI

  [processor.pytest]
  enabled_if.file = "pytest.ini"  # Only if pytest.ini exists

  [processor.integration_tests]
  enabled_if.branch = "main"      # Only on main branch

  [processor.slow_lint]
  enabled_if.command = "test -n \"$FULL_BUILD\""  # Custom condition
  ```
- Multiple conditions can be combined:
  ```toml
  [processor.deploy_check]
  enabled_if.all = [
    { env = "CI" },
    { branch = "main" },
    { file = ".deploy-ready" }
  ]
  ```
- `rsb processor list` shows which processors are enabled/disabled and why.
- This avoids needing multiple config files or complex shell scripts around rsb.

### Target aliases
- Define named groups of processors or products for easy invocation.
- Configuration:
  ```toml
  [alias]
  lint = ["ruff", "pylint", "shellcheck", "cpplint"]
  test = ["pytest", "doctest"]
  check = ["@lint", "@test", "mypy"]  # Aliases can reference other aliases
  fast = ["ruff", "template"]          # Quick feedback loop
  ```
- Usage:
  ```bash
  rsb build @lint          # Run only linting processors
  rsb build @test          # Run only test processors
  rsb build @check         # Run lint + test + mypy
  rsb build @fast          # Quick iteration
  ```
- Special aliases:
  - `@all` — All enabled processors (default)
  - `@changed` — Only processors with stale products
  - `@failed` — Re-run products that failed in the last build
- File-based targeting:
  ```bash
  rsb build src/main.c     # Build products that depend on this file
  rsb build src/           # Build products for all files in directory
  ```
- Combining aliases and files:
  ```bash
  rsb build @lint src/     # Lint only files in src/
  ```

## Graph & Query

### Build graph query language
- Bazel has `bazel query`, `cquery`, and `aquery` for exploring the dependency graph.
- rsb could support queries like:
  - `rsb query deps out/template/foo.py` — what does this product depend on?
  - `rsb query rdeps src/main.c` — what products are affected if this file changes?
  - `rsb query processor:ruff` — list all ruff products
- Useful for debugging builds and for CI systems that want to build only affected targets.

### Affected analysis
- Given a set of changed files (e.g., from `git diff`), determine which products are affected and only build those.
- Nx and Pants both feature this prominently.
- Useful for large projects where a full build is expensive but most changes only affect a subset.

## Extensibility

### ~~Lua plugin system~~ *(Done)*
- See [Lua Plugins](plugins.md).

### Plugin registry
- A central repository of community-contributed Lua plugins.
- Install plugins with a simple command:
  ```bash
  rsb plugin install eslint
  rsb plugin install prettier
  rsb plugin search typescript
  ```
- Plugins are downloaded to `plugins/` directory and automatically enabled.
- Registry could be a GitHub repository with a JSON index, or a dedicated service.
- Each plugin entry includes: name, description, author, version, dependencies (required tools), and the Lua source.
- Version pinning in `rsb.toml`:
  ```toml
  [plugins.registry]
  eslint = "1.2.0"
  prettier = "latest"
  ```
- `rsb plugin update` fetches newer versions.
- Security consideration: plugins execute arbitrary Lua code, so the registry should support signatures or checksums.

### Project templates
- Initialize new projects with pre-configured processors and directory structure.
- Templates for common project types:
  ```bash
  rsb init --template=python      # ruff, pylint, mypy, pytest
  rsb init --template=typescript  # eslint, prettier, tsc
  rsb init --template=cpp         # cc_single_file, cpplint, clang-format
  rsb init --template=rust        # rustfmt, clippy
  rsb init --template=docs        # spellcheck, markdownlint, pandoc
  ```
- Templates define: `rsb.toml` configuration, directory structure, example files, and `.gitignore` entries.
- Custom templates from local directories or URLs:
  ```bash
  rsb init --template=./my-template
  rsb init --template=https://github.com/user/rsb-template-web
  ```
- Templates are just directories with an `rsb-template.toml` manifest describing what to copy and what variables to substitute.

### Rule composition / aspects
- Bazel's "aspects" let you attach cross-cutting behavior to all targets of a certain type (e.g., "add coverage analysis to every C++ compile").
- rsb could support something similar — e.g., automatically lint everything that gets compiled.

## Developer Experience

### ~~JSON output mode~~ *(Done)*
- Machine-readable output for CI integration and tooling.
- Enable with the `--json` global flag:
  ```bash
  rsb build --json
  ```
- Output format (JSON Lines, one object per line):
  ```json
  {"event":"build_start","total_products":5}
  {"event":"product_start","product":"test.txt","processor":"template","inputs":["templates/test.txt.tera"],"outputs":["test.txt"]}
  {"event":"product_complete","product":"test.txt","processor":"template","status":"success","duration_ms":42}
  {"event":"product_complete","product":"main.py","processor":"ruff","status":"skipped"}
  {"event":"product_complete","product":"lib.py","processor":"ruff","status":"restored"}
  {"event":"product_complete","product":"bad.py","processor":"ruff","status":"failed","error":"E501 line too long"}
  {"event":"build_summary","total":5,"success":1,"failed":1,"skipped":1,"restored":1,"duration_ms":1234,"errors":["..."]}
  ```
- Status values: `success`, `failed`, `skipped` (unchanged), `restored` (from cache).
- When `--json` is enabled, human-readable output is suppressed.

### Build profiling / tracing
- Beyond `--timings`, generate a Chrome trace format or flamegraph SVG showing exactly what ran when, including parallel lanes.
- Bazel generates `--profile` output viewable in Chrome's `chrome://tracing`.
- Invaluable for diagnosing slow builds.
- Usage:
  ```bash
  rsb build --trace=build.json
  # Open chrome://tracing and load build.json
  ```
- Trace format shows: product start/end times, parallel execution lanes, wait times for dependencies.

### Build notifications
- Desktop notifications when builds complete, especially useful for long builds.
- Configuration:
  ```toml
  [build]
  notify = true              # Enable notifications
  notify_on_success = false  # Only notify on failure (default)
  notify_command = "notify-send"  # Custom command (default: platform-specific)
  ```
- Default behavior: notify on failure only, with summary ("Build failed: 3 errors in ruff").
- On Linux, uses `notify-send`. On macOS, uses `osascript`. On Windows, uses PowerShell toast.
- Also useful in watch mode: get notified when a rebuild completes after saving a file.

### Progress indicator
- For parallel builds, show a status line like `[3/17] Building... (2 running)` instead of just streaming output.
- Ninja and Buck2 both do this well.
- Implementation options:
  - Simple: single status line with product count and active jobs
  - Rich: progress bar with ETA based on historical build times
  - Interactive: TUI showing all active products in real-time
- The indicatif crate (already a dependency) provides progress bar primitives.
- Considerations: must handle interleaved output from parallel processors, should degrade gracefully when stdout is not a TTY.

### Actionable error messages
- When a product fails, show context: which processor, which input file, the exact command that was run.
- Include suggestions (e.g., "shellcheck not found — install with `apt install shellcheck`").

### Explain commands
- `rsb why <file>` — Explain why a file needs rebuilding:
  ```bash
  $ rsb why out/template/config.py
  out/template/config.py needs rebuild because:
    - Input templates/config.py.tera changed (mtime: 2024-01-15 10:30:00)
    - Input config/settings.py changed (checksum mismatch)
  ```
- `rsb deps <file>` — Show dependency tree for a product:
  ```bash
  $ rsb deps out/cc_single_file/main.elf
  out/cc_single_file/main.elf
  ├── src/main.c
  ├── src/utils.h (included by main.c)
  └── src/config.h (included by utils.h)
  ```
- These commands help debug unexpected rebuilds and understand the dependency graph.
- `rsb why` is especially useful when a file keeps rebuilding unexpectedly — it shows exactly which input triggered the rebuild.

### IDE / LSP integration
- Language Server Protocol (LSP) server for IDE integration.
- Features:
  - **Diagnostics**: Show build errors inline in the editor
  - **Code actions**: "Run rsb build" on save, "Clean this product"
  - **Hover info**: Show product status (up-to-date, stale, building)
  - **File decorations**: Mark files with build status icons
- Implementation: `rsb lsp` command starts an LSP server that IDEs connect to.
- Alternatively, provide plugins for popular editors:
  - VS Code extension
  - Neovim plugin (Lua)
  - Emacs package
- The LSP server would maintain a persistent connection to rsb, avoiding startup overhead.

### Build log capture
- Save stdout/stderr from each product execution to a log file.
- Useful for debugging failures, especially in CI where output scrolls away.
- Configuration:
  ```toml
  [build]
  log_dir = ".rsb/logs"  # Directory for build logs
  log_retention = 10     # Keep logs from last N builds
  ```
- Log file naming: `.rsb/logs/<build-id>/<processor>/<product>.log`
- `rsb log <product>` — View the log from the last build:
  ```bash
  rsb log ruff:main.py
  rsb log --build=2 ruff:main.py  # From 2 builds ago
  ```
- Logs are pruned automatically based on `log_retention`.

## Caching & Performance

### Native C/C++ include scanner
- Currently `cc_single_file` uses `gcc -MM` to discover header dependencies, which spawns an external process for each source file.
- A native Rust implementation could be significantly faster, especially for projects with many C/C++ files.
- Implementation approach:
  - Parse `#include` directives (both `"..."` and `<...>` forms)
  - Recursively follow includes, respecting `-I` include paths
  - Handle conditional compilation (`#ifdef`, `#ifndef`, `#if defined(...)`) where possible
  - Cache results aggressively since headers rarely change
- Existing crates to evaluate:
  - [shader-prepper](https://github.com/h3r2tic/shader-prepper) — Lightweight include scanner for shader files, doesn't implement full preprocessing, only `#include` scanning. Could be adapted for C/C++.
  - [mini-c-parser](https://crates.io/crates/mini-c-parser) — Full C lexer/parser in Rust, can extract preprocessor directives.
- Challenges:
  - Conditional compilation makes accurate dependency detection hard without full preprocessing
  - Computed includes (`#include MACRO`) require macro expansion
  - System headers need to be handled (skip or include based on configuration)
- Pragmatic approach: scan `#include` lines with regex, ignore conditionals, accept occasional false positives (extra rebuilds are safe, missed rebuilds are not)
- Configuration:
  ```toml
  [processor.cc_single_file]
  native_include_scanner = true  # Use native scanner instead of gcc -MM
  ```
- Fallback: if native scanner fails (e.g., computed include), fall back to `gcc -MM` for that file
- Benefits: faster dependency scanning, no compiler dependency for scanning phase, better parallelization

### Lazy file hashing (mtime-based)
- Currently rsb computes SHA-256 checksums for all input files on every build.
- For large repositories, this can be slow even when nothing has changed.
- Optimization: only re-hash files whose mtime has changed since the last build.
- Implementation:
  - Store `(path, mtime, checksum)` tuples in the cache database
  - On build, stat each file and compare mtime
  - Only compute checksum if mtime differs
  - Fall back to full hash if mtime resolution is insufficient (some filesystems have 1-second granularity)
- This is how Make works, but with checksums as the fallback for correctness.
- Risk: mtime can be unreliable (e.g., after `git checkout`, extracting archives, or on network filesystems). The `--force` flag should bypass this optimization.
- Configuration:
  ```toml
  [cache]
  mtime_cache = true  # Enable mtime-based caching (default: false)
  ```

### Compressed cache objects
- Compress cached objects to reduce disk usage and improve remote cache transfer times.
- Use zstd for fast compression/decompression with good ratios.
- Implementation:
  - Objects stored as `.zst` files in `.rsb/objects/`
  - Transparent compression/decompression in ObjectStore
  - Remote cache transfers compressed data directly
- Configuration:
  ```toml
  [cache]
  compression = "zstd"  # Options: "none", "zstd", "lz4"
  compression_level = 3  # zstd level (1-19, default 3)
  ```
- Trade-offs:
  - CPU cost for compression (mitigated by fast codecs like zstd/lz4)
  - Disk savings typically 50-80% for text files, less for binaries
  - Remote cache benefits most (network is usually slower than compression)
- The zstd crate provides a pure Rust implementation.

### Deferred materialization
- Don't write cached outputs to disk until they're actually needed by a downstream product or the final build result.
- For large graphs with deep caching, this avoids writing files that are never used.
- Buck2 does this aggressively.

### Garbage collection policy
- Currently `rsb cache trim` removes unreferenced objects.
- Add time-based or size-based policies: "keep cache under 1GB" or "evict entries older than 30 days."
- Useful for CI environments with limited disk.
- Configuration:
  ```toml
  [cache]
  max_size = "1GB"      # Maximum cache size
  max_age = "30d"       # Maximum age for cache entries
  gc_policy = "lru"     # Eviction policy: "lru" or "fifo"
  ```
- `rsb cache gc` — Run garbage collection manually
- Automatic GC after builds when cache exceeds limits

### Shared cache across branches
- When switching git branches, products built on another branch should be restorable from cache if their inputs match.
- This already works implicitly if the input hash matches, but it could be surfaced in `rsb status` ("restorable from branch X").

## Reproducibility

### Hermetic builds
- Ensure builds are completely reproducible by controlling all inputs, not just tool versions.
- Beyond tool version locking, hermetic builds would:
  - **Isolate environment variables**: Only pass explicitly declared env vars to processors
  - **Control timestamps**: Set deterministic mtimes on output files
  - **Sandbox network access**: Prevent processors from fetching external resources
  - **Pin system libraries**: Hash libc and other system dependencies
- Configuration:
  ```toml
  [build]
  hermetic = true
  allowed_env = ["HOME", "PATH", "CC", "CXX"]  # Env vars to pass through
  ```
- Hermetic mode would:
  - Clear environment except for allowed variables
  - Set `SOURCE_DATE_EPOCH` for reproducible timestamps
  - Optionally use Linux namespaces to restrict network/filesystem access
- Verification: `rsb build --verify` builds twice and compares outputs
- This is a spectrum — full hermeticity (like Bazel) requires significant infrastructure, but partial hermeticity still improves reproducibility.

### ~~Tool version locking~~ *(Done)*
- Each processor declares the tools it needs via `required_tools()` and how to query their version via `tool_version_commands()`.
- `rsb tools lock` queries each enabled processor's tools for their version, resolves the full binary path, and writes `.tools.versions` (JSON) to the project root. This file should be committed to version control.
- Lock file format includes schema version, timestamp, and per-tool entries with resolved path, version output, and the arguments used to obtain the version.
- On every `rsb build`, rsb reads `.tools.versions` and checks that installed tool versions match. Mismatch is a hard error by default; `--ignore-tool-versions` overrides this.
- If `.tools.versions` does not exist, `rsb build` warns and suggests running `rsb tools lock`.
- Tool versions are included in the cache key hash for each processor, so upgrading a tool and re-locking automatically invalidates cached outputs.
- Only tools for enabled processors are included in the lock file. Adding a new processor to `enabled` requires re-locking.
- Version comparison uses raw output strings (not parsed semver) to handle the wide variety of version output formats across tools.
- The lock file stores the resolved binary path so switching between system and local installs is detected.
- `rsb tools lock --check` verifies without writing. Bare `rsb tools lock` writes/updates the lock file.
- Inspired by Bazel's explicit toolchain management.

### Determinism verification
- A `rsb build --verify` mode that builds each product twice and compares outputs.
- If they differ, the build is non-deterministic.
- Bazel has `--experimental_check_output_files` for similar purposes.

## Security

### Shell command execution from source file comments
- `src/processors/cc.rs` — `EXTRA_*_SHELL` directives execute arbitrary shell commands parsed from source file comments.
- Document the security implications clearly.
