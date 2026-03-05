# Suggestions

Ideas for future improvements, organized by category.
Completed items have been moved to [suggestions-done.md](suggestions-done.md).

Grades:
- **Urgency**: `high` (users need this), `medium` (nice to have), `low` (speculative/future)
- **Complexity**: `low` (hours), `medium` (days), `high` (weeks+)

## Test Coverage

### Add tests for untested generators
- 14 out of 17 generator processors have no integration tests: a2x, drawio, gem, libreoffice, markdown, marp, mermaid, npm, pandoc, pdflatex, pdfunite, pip, sphinx.
- The test pattern is well-established in `tests/processors/` — each test creates a temp project, writes source files, runs `rsbuild build`, and verifies outputs.
- **Urgency**: high | **Complexity**: low (per processor)

### Add tests for untested checkers
- 5 checkers have no integration tests: ascii_check, aspell, markdownlint, mdbook, mdl.
- **Urgency**: medium | **Complexity**: low (per processor)

## New Processors

### Linting / Checking (stub-based)

#### yamllint
- Lint YAML files (`.yml`, `.yaml`) using `yamllint`.
- Catches syntax errors and style violations.
- Config: `linter` (default `"yamllint"`), `args`, `extra_inputs`, `scan`.
- **Urgency**: medium | **Complexity**: low

#### jsonlint
- Validate JSON files (`.json`) for syntax errors.
- Could use `python3 -m json.tool` or a dedicated tool like `jsonlint`.
- Config: `linter`, `args`, `extra_inputs`, `scan`.
- **Urgency**: medium | **Complexity**: low

#### toml-lint
- Validate TOML files (`.toml`) for syntax errors.
- Could use `taplo check` or a built-in Rust parser.
- Config: `linter` (default `"taplo"`), `args`, `extra_inputs`, `scan`.
- **Urgency**: low | **Complexity**: low

#### markdownlint
- Lint Markdown files (`.md`) for structural issues (complements spellcheck which only checks spelling).
- Uses `mdl` or `markdownlint-cli`.
- Config: `linter` (default `"mdl"`), `args`, `extra_inputs`, `scan`.
- **Urgency**: low | **Complexity**: low

#### black-check
- Python formatting verification using `black --check`.
- Verifies files are formatted without modifying them.
- Config: `args`, `extra_inputs`, `scan`.
- **Urgency**: low | **Complexity**: low

### Compilation / Generation

#### rust_single_file
- Compile single-file Rust programs (`.rs`) to executables, like cc_single_file but for Rust.
- Useful for exercise/example repositories.
- Config: `rustc` (default `"rustc"`), `flags`, `output_suffix`, `extra_inputs`, `scan`.
- **Urgency**: medium | **Complexity**: medium

#### sass
- Compile `.scss`/`.sass` files to `.css`.
- Single-file transformation using `sass` or `dart-sass`.
- Config: `compiler` (default `"sass"`), `args`, `extra_inputs`, `scan`.
- **Urgency**: low | **Complexity**: low

#### protobuf
- Compile `.proto` files to generated code using `protoc`.
- Config: `protoc` (default `"protoc"`), `args`, `language` (default `"cpp"`), `extra_inputs`, `scan`.
- **Urgency**: low | **Complexity**: medium

#### pandoc
- Convert Markdown (`.md`) to other formats (PDF, HTML, EPUB) using `pandoc`.
- Single-file transformation.
- Config: `output_format` (default `"html"`), `args`, `extra_inputs`, `scan`.
- **Urgency**: low | **Complexity**: low

#### jinja2
- Render Jinja2 templates (`.j2`, `.jinja2`) via `python3 -c` using the `jinja2` library.
- Similar to the mako and tera processors but using Jinja2 syntax.
- Scan directory: `templates.jinja2/`, strips prefix and extension for output path.
- Config: `extra_inputs`, `scan`.
- **Urgency**: medium | **Complexity**: low

### Testing

#### pytest
- Run Python test files and produce pass/fail stubs.
- Each `test_*.py` file becomes a product.
- Config: `runner` (default `"pytest"`), `args`, `extra_inputs`, `scan` (default extensions `["test_*.py"]`).
- **Urgency**: medium | **Complexity**: medium

#### doctest
- Run Python doctests and produce stubs.
- Each `.py` file with doctests produces a stub.
- Config: `args`, `extra_inputs`, `scan`.
- **Urgency**: low | **Complexity**: medium

## Build Execution

### Distributed builds
- Run builds across multiple machines, similar to distcc or icecream for C/C++.
- A coordinator node distributes work to worker nodes, each running rsbuild in worker mode.
- Workers execute products and return outputs to the coordinator, which caches them locally.
- Challenges: network overhead for small products, identical tool versions across workers, local filesystem access.
- **Urgency**: low | **Complexity**: high

### Sandboxed execution
- Run each processor in an isolated environment where it can only access its declared inputs.
- Prevents accidental undeclared dependencies.
- On Linux, namespaces can provide lightweight sandboxing.
- **Urgency**: low | **Complexity**: high

### Content-addressable outputs (unchanged output pruning)
- Hash outputs too to skip downstream rebuilds when an input changes but produces identical output.
- Bazel calls this "unchanged output pruning."
- **Urgency**: medium | **Complexity**: medium

### Persistent daemon mode
- Keep rsbuild running as a background daemon to avoid startup overhead.
- Benefits: instant file index via inotify, warm Lua VMs, connection pooling, faster incremental builds.
- Daemon listens on Unix socket (`.rsbuild/daemon.sock`).
- `rsbuild watch` becomes a client that triggers rebuilds on file events.
- **Urgency**: low | **Complexity**: high

### Persistent workers
- Keep long-running tool processes alive to avoid startup overhead.
- Instead of spawning `ruff` or `pylint` per invocation, keep one process alive and feed it files.
- Bazel gets 2-4x speedup for Java this way. Could benefit pylint/mypy which have heavy startup.
- Multiplex variant: multiple requests to a single worker process via threads.
- **Urgency**: medium | **Complexity**: high

### Dynamic execution (race local vs remote)
- Start both local and remote execution of the same product; use whichever finishes first and cancel the other.
- Useful when remote cache is slow or flaky.
- Configurable per-processor via execution strategy.
- **Urgency**: low | **Complexity**: high

### Execution strategies per processor
- Map each processor to an execution strategy: local, remote, sandboxed, or dynamic.
- Different processors may benefit from different execution models.
- Config: `[processor.ruff] execution = "remote"`, `[processor.cc_single_file] execution = "sandboxed"`.
- **Urgency**: low | **Complexity**: medium

### Build profiles
- Named configuration sets for different build scenarios (ci, dev, release).
- Profiles inherit from base configuration and override specific values.
- Usage: `rsbuild build --profile=ci`
- **Urgency**: medium | **Complexity**: medium

### Conditional processors
- Enable or disable processors based on conditions (environment variables, file existence, git branch, custom commands).
- Multiple conditions can be combined with `all`/`any` logic.
- **Urgency**: low | **Complexity**: medium

### Target aliases
- Define named groups of processors for easy invocation.
- Usage: `rsbuild build @lint`, `rsbuild build @test`
- Special aliases: `@all`, `@changed`, `@failed`
- File-based targeting: `rsbuild build src/main.c`
- **Urgency**: medium | **Complexity**: medium

## Graph & Query

### Build graph query language
- Support queries like `rsbuild query deps out/foo`, `rsbuild query rdeps src/main.c`, `rsbuild query processor:ruff`.
- Useful for debugging builds and CI systems that want to build only affected targets.
- **Urgency**: low | **Complexity**: medium

### Affected analysis
- Given changed files (from `git diff`), determine which products are affected and only build those.
- Useful for large projects where a full build is expensive.
- **Urgency**: medium | **Complexity**: medium

### Critical path analysis
- Identify the longest sequential chain of actions in a build.
- Helps users optimize their slowest builds by showing what's actually on the critical path.
- Display with `rsbuild build --critical-path` or include in `--timings` output.
- **Urgency**: medium | **Complexity**: medium

## Extensibility

### Plugin registry
- A central repository of community-contributed Lua plugins.
- Install with `rsbuild plugin install eslint`.
- Registry could be a GitHub repository with a JSON index.
- Version pinning in `rsbuild.toml`.
- **Urgency**: low | **Complexity**: high

### Project templates
- Initialize new projects with pre-configured processors and directory structure.
- `rsbuild init --template=python`, `rsbuild init --template=cpp`, etc.
- Custom templates from local directories or URLs.
- **Urgency**: low | **Complexity**: medium

### Rule composition / aspects
- Attach cross-cutting behavior to all targets of a certain type (e.g., "add coverage analysis to every C++ compile").
- **Urgency**: low | **Complexity**: high

### Output groups / subtargets
- Named subsets of a target's outputs that can be requested selectively.
- E.g., `rsbuild build --output-group=debug` or per-product subtarget selection.
- Useful for targets that produce multiple output types (headers, binaries, docs).
- **Urgency**: low | **Complexity**: medium

### Visibility / access control
- Restrict which processors can consume which files or directories.
- Prevents accidental cross-boundary dependencies in large repos.
- Config: per-processor `visibility` rules or directory-level `.rsbuild-visibility` files.
- **Urgency**: low | **Complexity**: medium

## Developer Experience

### Build profiling / tracing
- Generate Chrome trace format or flamegraph SVG showing what ran when, including parallel lanes.
- Include critical path highlighting, CPU usage, and idle time analysis.
- Usage: `rsbuild build --trace=build.json`
- Viewable in `chrome://tracing` or Perfetto UI.
- **Urgency**: medium | **Complexity**: medium

### Build Event Protocol / structured event stream
- rsbuild has `--json` on stdout, but a proper Build Event Protocol (file or gRPC stream) enables external dashboards, CI integrations, and build analytics services.
- Write events to a file (`--build-event-log=events.pb`) or stream to a remote service.
- Richer event types than current JSON Lines: action graph, configuration, progress, test results.
- **Urgency**: medium | **Complexity**: medium

### Build notifications
- Desktop notifications when builds complete, especially for long builds.
- Platform-specific: `notify-send` (Linux), `osascript` (macOS).
- Config: `notify = true`, `notify_on_success = false`.
- **Urgency**: low | **Complexity**: low

### `rsbuild build <target>` — Build specific targets
- Build only specific targets by name or pattern:
  `rsbuild build src/main.c`, `rsbuild build out/cc_single_file/`, `rsbuild build "*.py"`
- **Urgency**: medium | **Complexity**: medium

### Parallel dependency analysis
- The cpp analyzer scans files sequentially, which can be slow for large codebases.
- Parallelize header scanning using rayon or tokio.
- **Urgency**: low | **Complexity**: medium

### IDE / LSP integration
- Language Server Protocol server for IDE integration.
- Features: diagnostics, code actions, hover info, file decorations.
- Plugins for VS Code, Neovim, Emacs.
- **Urgency**: low | **Complexity**: high

### Build log capture
- Save stdout/stderr from each product execution to a log file.
- Config: `log_dir = ".rsbuild/logs"`, `log_retention = 10`.
- `rsbuild log ruff:main.py` to view logs.
- **Urgency**: low | **Complexity**: medium

### Build timing history
- Store timing data to `.rsbuild/timings.json` after each build.
- `rsbuild timings` shows slowest products, trends, time per processor.
- **Urgency**: low | **Complexity**: medium

### Remote cache authentication
- Support authenticated remote caches: S3 (AWS credentials), HTTP (bearer tokens), GCS.
- Variable substitution from environment for secrets.
- **Urgency**: medium | **Complexity**: medium

### `rsbuild fmt` — Auto-format source files
- Run formatters (black, isort, clang-format, rustfmt) that modify files in-place.
- Distinct from checkers which only verify — formatters actually fix formatting.
- Could be a new processor type (`Formatter`) or a convenience command that runs formatter processors.
- **Urgency**: medium | **Complexity**: medium

### `rsbuild why <file>` — Explain why a file is built
- Show which processors consume a given file, what products it belongs to, and what triggered a rebuild.
- Useful for debugging unexpected rebuilds or understanding the build graph.
- **Urgency**: medium | **Complexity**: low

### `rsbuild doctor` — Diagnose build environment
- Check for common issues: missing tools, misconfigured processors, stale cache, version mismatches.
- Report warnings and suggestions for fixing problems.
- **Urgency**: medium | **Complexity**: low

### `rsbuild lint` — Run only checkers
- Convenience command to run only checker processors.
- Equivalent to `rsbuild build -p ruff,pylint,...` but shorter.
- **Urgency**: low | **Complexity**: low

### `rsbuild sloc` — Source lines of code statistics
- Count source lines of code across the project, broken down by language/extension.
- Leverage rsbuild's existing file index and extension-to-language mapping from processor configs.
- Show: files, blank lines, comment lines, code lines per language. Total summary.
- Optional COCOMO-style effort/cost estimation (person-months, schedule, cost at configurable salary).
- Usage: `rsbuild sloc`, `rsbuild sloc --json`, `rsbuild sloc --cocomo --salary 100000`
- Similar to external tools: `sloccount`, `cloc`, `tokei`.
- **Urgency**: low | **Complexity**: medium

### Watch mode keyboard commands
- During `rsbuild watch`, support `r` (rebuild), `c` (clean), `q` (quit), `Enter` (rebuild now), `s` (status).
- Only activate when stdin is a TTY.
- **Urgency**: low | **Complexity**: medium

### Layered config files
- Support config file layering: system (`/etc/rsbuild/config.toml`), user (`~/.config/rsbuild/config.toml`), project (`rsbuild.toml`).
- Lower layers provide defaults, higher layers override.
- Per-command overrides via `[build]`, `[watch]` sections.
- Similar to Bazel's `.bazelrc` layering.
- **Urgency**: low | **Complexity**: low

### Test sharding
- Split large test targets across multiple parallel shards.
- Set `TEST_TOTAL_SHARDS` and `TEST_SHARD_INDEX` environment variables for test runners.
- Config: `shard_count = 4` per processor or product.
- Useful for pytest/doctest processors when added.
- **Urgency**: low | **Complexity**: medium

### Runfiles / runtime dependency trees
- Track runtime dependencies (shared libs, config files, data files) separately from build dependencies.
- Generate a runfiles directory per executable with symlinks to all transitive runtime deps.
- Useful for deployment, packaging, and containerization.
- **Urgency**: low | **Complexity**: high

## Caching & Performance

### Deferred materialization
- Don't write cached outputs to disk until they're actually needed by a downstream product.
- **Urgency**: low | **Complexity**: high

### Garbage collection policy
- Time-based or size-based cache policies: "keep cache under 1GB" or "evict entries older than 30 days."
- Config: `max_size = "1GB"`, `max_age = "30d"`, `gc_policy = "lru"`.
- `rsbuild cache gc` for manual garbage collection.
- **Urgency**: low | **Complexity**: medium

### Shared cache across branches
- Surface in `rsbuild status` when products are restorable from another branch.
- Already works implicitly via input hash matching.
- **Urgency**: low | **Complexity**: low

### Merkle tree input hashing
- Hash inputs as a Merkle tree rather than flat concatenation.
- More efficient for large input sets — changing one file only rehashes its branch, not all inputs.
- Also enables efficient transfer of input trees to remote execution workers.
- **Urgency**: low | **Complexity**: medium

## Reproducibility

### Hermetic builds
- Control all inputs beyond tool versions: isolate env vars, control timestamps, sandbox network, pin system libraries.
- Config: `hermetic = true`, `allowed_env = ["HOME", "PATH"]`.
- Verification: `rsbuild build --verify` builds twice and compares outputs.
- **Urgency**: low | **Complexity**: high

### Determinism verification
- `rsbuild build --verify` mode that builds each product twice and compares outputs.
- **Urgency**: low | **Complexity**: medium

## Security

### Shell command execution from source file comments
- `EXTRA_*_SHELL` directives execute arbitrary shell commands parsed from source file comments.
- Document the security implications clearly.
- **Urgency**: medium | **Complexity**: low
