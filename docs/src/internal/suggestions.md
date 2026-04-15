# Suggestions

Ideas for future improvements, organized by category.
Completed items have been moved to [suggestions-done.md](suggestions-done.md).

Grades:
- **Urgency**: `high` (users need this), `medium` (nice to have), `low` (speculative/future)
- **Complexity**: `low` (hours), `medium` (days), `high` (weeks+)

## Build Execution

### Distributed builds
- Run builds across multiple machines, similar to distcc or icecream for C/C++.
- A coordinator node distributes work to worker nodes, each running rsconstruct in worker mode.
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
- Keep rsconstruct running as a background daemon to avoid startup overhead.
- Benefits: instant file index via inotify, warm Lua VMs, connection pooling, faster incremental builds.
- Daemon listens on Unix socket (`.rsconstruct/daemon.sock`).
- `rsconstruct watch` becomes a client that triggers rebuilds on file events.
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
- Usage: `rsconstruct build --profile=ci`
- **Urgency**: medium | **Complexity**: medium

### Conditional processors
- Enable or disable processors based on conditions (environment variables, file existence, git branch, custom commands).
- Multiple conditions can be combined with `all`/`any` logic.
- **Urgency**: low | **Complexity**: medium

### Target aliases
- Define named groups of processors for easy invocation.
- Usage: `rsconstruct build @lint`, `rsconstruct build @test`
- Special aliases: `@all`, `@changed`, `@failed`
- File-based targeting: `rsconstruct build src/main.c`
- **Urgency**: medium | **Complexity**: medium

## Graph & Query

### Build graph query language
- Support queries like `rsconstruct query deps out/foo`, `rsconstruct query rdeps src/main.c`, `rsconstruct query processor:ruff`.
- Useful for debugging builds and CI systems that want to build only affected targets.
- **Urgency**: low | **Complexity**: medium

### Affected analysis
- Given changed files (from `git diff`), determine which products are affected and only build those.
- Useful for large projects where a full build is expensive.
- **Urgency**: medium | **Complexity**: medium

### Critical path analysis
- Identify the longest sequential chain of actions in a build.
- Helps users optimize their slowest builds by showing what's actually on the critical path.
- Display with `rsconstruct build --critical-path` or include in `--timings` output.
- **Urgency**: medium | **Complexity**: medium

## Extensibility

### Plugin registry
- A central repository of community-contributed Lua plugins.
- Install with `rsconstruct plugin install eslint`.
- Registry could be a GitHub repository with a JSON index.
- Version pinning in `rsconstruct.toml`.
- **Urgency**: low | **Complexity**: high

### Project templates
- Initialize new projects with pre-configured processors and directory structure.
- `rsconstruct init --template=python`, `rsconstruct init --template=cpp`, etc.
- Custom templates from local directories or URLs.
- **Urgency**: low | **Complexity**: medium

### Rule composition / aspects
- Attach cross-cutting behavior to all targets of a certain type (e.g., "add coverage analysis to every C++ compile").
- **Urgency**: low | **Complexity**: high

### Output groups / subtargets
- Named subsets of a target's outputs that can be requested selectively.
- E.g., `rsconstruct build --output-group=debug` or per-product subtarget selection.
- Useful for targets that produce multiple output types (headers, binaries, docs).
- **Urgency**: low | **Complexity**: medium

### Visibility / access control
- Restrict which processors can consume which files or directories.
- Prevents accidental cross-boundary dependencies in large repos.
- Config: per-processor `visibility` rules or directory-level `.rsconstruct-visibility` files.
- **Urgency**: low | **Complexity**: medium

## Developer Experience

### Build Event Protocol / structured event stream
- rsconstruct already has `--json` on stdout with JSON Lines events (BuildEvent, ProductStart, ProductComplete, BuildSummary) and `--trace` for Chrome trace format.
- A proper Build Event Protocol (file or gRPC stream) would enable external dashboards, CI integrations, and build analytics services beyond what JSON Lines provides.
- Write events to a file (`--build-event-log=events.pb`) or stream to a remote service.
- Richer event types: action graph, configuration, progress, test results.
- **Urgency**: medium | **Complexity**: medium

### Build notifications
- Desktop notifications when builds complete, especially for long builds.
- Platform-specific: `notify-send` (Linux), `osascript` (macOS).
- Config: `notify = true`, `notify_on_success = false`.
- **Urgency**: low | **Complexity**: low

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
- Config: `log_dir = ".rsconstruct/logs"`, `log_retention = 10`.
- `rsconstruct log ruff:main.py` to view logs.
- **Urgency**: low | **Complexity**: medium

### Build timing history
- Store timing data to `.rsconstruct/timings.json` after each build.
- `rsconstruct timings` shows slowest products, trends, time per processor.
- **Urgency**: low | **Complexity**: medium

### Remote cache authentication
- S3 and HTTP/HTTPS remote caches are already supported.
- Still needed: explicit bearer token support, GCS backend, and environment variable substitution for secrets in config.
- **Urgency**: medium | **Complexity**: medium

### `rsconstruct fmt` — Auto-format source files
- Run formatters (black, isort, clang-format, rustfmt) that modify files in-place.
- Distinct from checkers which only verify — formatters actually fix formatting.
- Could be a new processor type (`Formatter`) or a convenience command that runs formatter processors.
- **Urgency**: medium | **Complexity**: medium

### `rsconstruct lint` — Run only checkers
- Convenience command to run only checker processors.
- Equivalent to `rsconstruct build -p ruff,pylint,...` but shorter.
- **Urgency**: low | **Complexity**: low

### Watch mode keyboard commands
- During `rsconstruct watch`, support `r` (rebuild), `c` (clean), `q` (quit), `Enter` (rebuild now), `s` (status).
- Only activate when stdin is a TTY.
- **Urgency**: low | **Complexity**: medium

### Layered config files
- Support config file layering: system (`/etc/rsconstruct/config.toml`), user (`~/.config/rsconstruct/config.toml`), project (`rsconstruct.toml`).
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

### On-demand processors (`build_by_default = false`)
- Today every declared processor runs on every `rsconstruct build`. The only per-invocation escape hatches are `-x name` (remember every time) or `enabled = false` in the config (remember to flip back). Neither fits the "this processor exists, don't run it unless I ask" use case — common for slow lifecycle processors like `python_package`, `docker_build`, `publish`, `release_tarball`.
- Add a per-processor boolean field defaulting to true: `build_by_default = false` on a processor means it's discovered and classified like any other, but its products are filtered out of the default run.
- Prior art: meson's `build_by_default: false`, Bazel's `tags = ["manual"]`, buck2's `tags = ["manual"]`. All use the same shape — declarative opt-out on the rule, per-invocation opt-in via target naming.
- CLI semantics map cleanly onto existing `-p`/`-x` machinery:
  - `rsconstruct build` → excludes `build_by_default = false` processors (new behaviour).
  - `rsconstruct build -p python_package` → includes only `python_package`; the `-p` explicit inclusion overrides the default-off flag.
  - `rsconstruct build -p ruff,python_package` → includes both, including the opt-in one.
  - `rsconstruct build --all` (new flag) → includes everything including on-demand processors. Useful for CI that wants to verify the opt-in path doesn't bitrot.
- Example config:
  ```toml
  [processor.python_package]
  build_by_default = false
  src_dirs = ["."]
  ```
- Design considerations:
  - **`@all` meta-shortcut:** the existing `@checkers` / `@generators` aliases should continue to mean "all of that type, subject to the default-off filter." Users who want "all checkers including on-demand ones" would say `rsconstruct build --all -p @checkers` — rare enough that the composition is fine.
  - **Error on contradiction:** `-p X -x X` already errors; `-p X` where X has `build_by_default = false` should just work (explicit opt-in wins over declarative opt-out).
  - **Watch mode:** `rsconstruct watch` should honour the same default — don't rebuild the package processor on every file save. Users who want watch-mode packaging can add `-p python_package` to the watch invocation.
  - **Discovery cost:** on-demand processors still run discovery every build, because we need to know what their products would be (for output-conflict detection, graph completeness, and `--all` support). This is negligible — discovery is O(files matched), not O(cost of running).
- Follow-up idea: **named goals** (meson-style aggregated targets or npm-style scripts) for the "I want a lint goal / deploy goal / ci goal" pattern. That's Pattern B, layered above per-processor config — not needed to solve the basic on-demand case.
- **Urgency**: medium | **Complexity**: low

### Decomposed cache key for richer `--explain`
- Today every product has a single descriptor key that mixes input checksum + config hash + tool-version hash + variant. A miss tells us "the key changed" but not *which component*. `--explain` can only say `BUILD (no cache entry)` / `BUILD (output missing)` — not "your cflags changed" or "an input file changed".
- Store the three sub-hashes (input, config, tool) in a new redb table keyed by stable product identity — `(processor_iname, primary_path)` where `primary_path` is the first output for generators or the first input for checkers.
- Schema: `product_components: (processor, primary_path) -> { input_hash, config_hash, tool_hash, timestamp }`. ~100 bytes per product, so ~500KB extra disk for a 5000-product project.
- **Reads only on `--explain`.** `classify_products` already routes through `explain_descriptor`; extend that to look up the prior components row, recompute current components, diff the three, and return a richer reason like `BUILD (config changed: cflags, include_paths)`.
- **Writes only when explicitly tracking.** Two reasonable gates:
  - **Option A (single flag):** `--explain` enables both write and read. CI runs without `--explain` → zero overhead. Trade-off: the first explain run after enabling has no prior row → reports "no prior state" generically. Subsequent runs work fully.
  - **Option B (separate `--track-changes` / `[build] track_changes = true`):** decouples capture from query. CI omits the flag → zero overhead. Devs opt in permanently via config.
  - Lean Option A: fewer flags, the existing `--explain` carries both ends of the lifecycle, and CI/CD pays nothing by default since neither flag is set.
- **Tier 1 only.** Says "input bucket changed" but not which file. For a `.cc` file with 100 headers, the user still doesn't know which header. A future Tier 2 (per-input-file checksums) would resolve that at ~5-10x storage cost; defer until users ask.
- **Caveats:** adds a third source of truth (alongside `descriptors` and the in-memory graph) to keep in sync. Stale entries (products dropped from config) accumulate harmlessly until `cache clear`.
- **Urgency**: medium | **Complexity**: medium

## Caching & Performance

### Deferred materialization
- Don't write cached outputs to disk until they're actually needed by a downstream product.
- **Urgency**: low | **Complexity**: high

### Garbage collection policy
- Time-based or size-based cache policies: "keep cache under 1GB" or "evict entries older than 30 days."
- Config: `max_size = "1GB"`, `max_age = "30d"`, `gc_policy = "lru"`.
- `rsconstruct cache gc` for manual garbage collection.
- **Urgency**: low | **Complexity**: medium

### Shared cache across branches
- Surface in `rsconstruct status` when products are restorable from another branch.
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
- Verification: `rsconstruct build --verify` builds twice and compares outputs.
- **Urgency**: low | **Complexity**: high

### Determinism verification
- `rsconstruct build --verify` mode that builds each product twice and compares outputs.
- **Urgency**: low | **Complexity**: medium

## CI & Reporting

### CI config generator
- `rsconstruct ci generate` outputs a GitHub Actions or GitLab CI config that runs the build.
- Detects enabled processors and required tools, generates install steps and build commands.
- Supports `--format=github|gitlab|circleci`.
- **Urgency**: medium | **Complexity**: medium

### HTML build report
- Generate a visual HTML dashboard of build times, cache hit rates, and processor statistics.
- `rsconstruct build --report=build.html` or `rsconstruct report`.
- Include charts for timing trends, per-processor breakdown, cache efficiency.
- **Urgency**: low | **Complexity**: medium

### PR comment bot
- Post build results (pass/fail, timing, warnings) as a GitHub PR comment.
- `rsconstruct ci comment` reads build output and posts via GitHub API.
- **Urgency**: low | **Complexity**: medium

## Content & Documentation

### `rsconstruct init --detect`
- `rsconstruct smart auto` already scans and enables processors, but a dedicated `init --detect` could go further.
- Generate a complete `rsconstruct.toml` with processor-specific config (src_dirs, extensions, tool paths).
- **Urgency**: medium | **Complexity**: medium

### `rsconstruct fmt`  — Auto-format rsconstruct.toml
- Sort `[processor.*]` sections alphabetically, align values, remove redundant defaults.
- Distinct from the existing `rsconstruct fmt` suggestion about formatting source files.
- **Urgency**: low | **Complexity**: low

### Cross-project term sync
- Automatically keep terms directories in sync across multiple repos.
- Could run as a daemon or a periodic CI job.
- `rsconstruct terms sync --repos=repo1,repo2` or config-driven.
- **Urgency**: low | **Complexity**: medium

### Glossary generator
- `rsconstruct terms glossary` generates a markdown glossary from the terms directory.
- Optionally pulls definitions from context in the markdown files where terms are used.
- **Urgency**: low | **Complexity**: medium

### Link checker processor
- Validate that URLs in markdown files are not broken (HTTP HEAD requests).
- Configurable timeout, retry, and allow/blocklist patterns.
- Cache results to avoid re-checking unchanged URLs.
- **Urgency**: medium | **Complexity**: medium

### Image optimizer processor
- Compress and resize images referenced in markdown files.
- Uses tools like `optipng`, `jpegoptim`, `svgo`.
- Config: quality levels, max dimensions, output format.
- **Urgency**: low | **Complexity**: medium

### HTML+JS compression and packaging
- Minify and bundle HTML, CSS, and JavaScript files for deployment.
- Could use tools like `terser` (JS), `csso` (CSS), `html-minifier` (HTML).
- Bundle multiple JS/CSS files into single outputs, generate source maps.
- Integrate with existing eslint/stylelint processors for a full web frontend pipeline.
- **Urgency**: medium | **Complexity**: medium

## Processor Ecosystem

### WASM processor plugins
- Beyond Lua, allow processors written in any language compiled to WebAssembly.
- Provides sandboxing, portability, and language flexibility.
- WASI for filesystem access within the sandbox.
- **Urgency**: low | **Complexity**: high

### Processor marketplace / registry
- A central repository of community-contributed processor configs and Lua plugins.
- Install with `rsconstruct plugin install prettier`.
- Registry as a GitHub repository with a JSON index. Version pinning in `rsconstruct.toml`.
- **Urgency**: low | **Complexity**: high

## Cleaning & Cache

### Selective processor cleaning
- `rsconstruct clean outputs --processors ruff,pylint` to clean only specific processors' outputs.
- Currently `clean outputs` is all-or-nothing. Multi-processor projects need granular control.
- Filter products in the clean loop by processor name.
- **Urgency**: high | **Complexity**: low

### Time-based cache purge
- `rsconstruct cache purge --older-than=7d` to remove cache entries older than a given duration.
- Currently only `cache clear` exists which removes everything.
- Walk the object store, check file mtimes, remove old entries.
- **Urgency**: medium | **Complexity**: low

### Enhanced cache statistics
- `rsconstruct cache stats` currently shows minimal info.
- Add: hit rate percentage, bytes saved vs rebuild time, per-processor breakdown, slowest processors.
- Helps users identify optimization opportunities.
- **Urgency**: medium | **Complexity**: medium

## CLI & UX

### `rsconstruct status --json`
- The status command has no JSON output mode, unlike most other commands.
- CI systems can't parse the current human-readable output.
- Add JSON output with per-processor and total counts.
- **Urgency**: high | **Complexity**: low

### `rsconstruct processors search <keyword>`
- Search the processor list by name or description substring.
- With 85+ processors, scrolling through `processors list` is unwieldy.
- **Urgency**: medium | **Complexity**: low

### Config validation warnings
- Warn about common mistakes during build: processor enabled but no matching files, unknown fields, deprecated options.
- Passive warnings (not errors) shown before the build starts.
- We have `smart remove-no-file-processors` for cleanup, but no passive heads-up.
- **Urgency**: medium | **Complexity**: low

## Configuration

### Environment variable expansion in config
- Allow `${env:HOME}` or `${env:CI}` in `rsconstruct.toml` to reference environment variables.
- The variable substitution system already exists for `[vars]`; extending it to env vars is natural.
- Useful for CI/CD systems that pass secrets or paths via environment.
- **Urgency**: medium | **Complexity**: low

### Per-processor batch size
- Each processor config has a `batch` boolean, but batch size is global (`[build] batch_size`).
- Different tools have different startup costs — fast tools benefit from large batches, slow tools from small ones.
- Add `batch_size` field to individual processor configs, overriding the global default.
- **Urgency**: medium | **Complexity**: low

## Processor Ecosystem

### Prettier (JavaScript/TypeScript/CSS/HTML formatter)
- The most popular web formatter. Industry standard for frontend projects.
- Checker processor using `prettier --check`. Batch-capable.
- **Urgency**: high | **Complexity**: low

### Isort (Python import sorter)
- Complements ruff/black for complete Python formatting pipeline.
- Checker processor using `isort --check-only --diff`. Batch-capable.
- **Urgency**: medium | **Complexity**: low

### Flake8 (Python linter)
- Many projects still use flake8 over ruff. Widely adopted.
- Checker processor using `flake8`. Batch-capable.
- **Urgency**: medium | **Complexity**: low

## Security

### Shell command execution from source file comments
- `EXTRA_*_SHELL` directives execute arbitrary shell commands parsed from source file comments.
- Document the security implications clearly.
- **Urgency**: medium | **Complexity**: low
