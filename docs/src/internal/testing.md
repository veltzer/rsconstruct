# Testing

RSConstruct uses two kinds of tests:

1. **Integration tests** in `tests/` — the primary test suite. These exercise the compiled `rsconstruct` binary as a black box, building fake projects in temp directories and asserting on CLI output and side effects.
2. **Unit tests** in `src/` (`#[cfg(test)] mod tests`) — used sparingly, only for self-contained modules whose internals cannot be exercised adequately through the CLI. Currently this is `src/graph.rs` (dedup and topological-sort logic).

## Running tests

```bash
cargo test              # Run all tests
cargo test rsconstructignore    # Run tests matching a name
cargo test -- --nocapture  # Show stdout/stderr from tests
```

## Why unit tests live in `src/` (not `tests/`)

There is a recurring question: should unit tests move to `tests/` to keep source files shorter and more readable? The short answer is no, for a structural reason specific to this crate.

**This crate is a binary only — there is no `src/lib.rs`.** Integration tests under `tests/` can only link against a *library* crate; against a binary crate they can only do what `tests/main.rs` does today: spawn the `rsconstruct` binary as a subprocess and assert on its output. So there are only three real options for testing internal logic like `BuildGraph`:

| Option | Cost |
|---|---|
| Unit tests inline in `src/` (current) | Longer source files (mitigated by `#[cfg(test)]` stripping them from release builds, and by editor folding) |
| Move tests to `tests/` as end-to-end tests | Far more code per test, much slower, indirect — can't isolate a specific dedup branch without building a whole fake project |
| Add a `src/lib.rs` exposing modules | Architectural change — the crate becomes both a library and a binary. Forces decisions about what is public API |

The third option is the "clean" fix but it has ongoing costs (API surface to maintain, semver implications if we ever publish the library). The first option has only a readability cost, and it's the idiomatic Rust approach for binary crates.

**Rule:** default to writing integration tests in `tests/`. Only add a `#[cfg(test)] mod tests` block in `src/` when the thing under test is genuinely hard to exercise through the CLI (e.g. a specific branch of a dedup helper that requires setting up graph state that would take dozens of real products to reproduce end-to-end). When a source file grows large enough that its inline test module dominates the file, split the tests into a sibling file via `#[cfg(test)] mod tests;` + `src/MODULE/tests.rs`, rather than moving them out of `src/` entirely.

## Test directory layout

```
tests/
├── common/
│   └── mod.rs                  # Shared helpers (not a test binary)
├── build.rs                    # Build command tests
├── cache.rs                    # Cache operation tests
├── complete.rs                 # Shell completion tests
├── config.rs                   # Config show/show-default tests
├── dry_run.rs                  # Dry-run flag tests
├── graph.rs                    # Dependency graph tests
├── init.rs                     # Project initialization tests
├── processor_cmd.rs            # Processor list/auto/files tests
├── rsconstructignore.rs                # .rsconstructignore / .gitignore exclusion tests
├── status.rs                   # Status command tests
├── tools.rs                    # Tools list/check tests
├── watch.rs                    # File watcher tests
├── processors.rs               # Module root for processor tests
└── processors/
    ├── cc_single_file.rs       # C/C++ compilation tests
    ├── zspell.rs           # Zspell processor tests
    └── template.rs             # Template processor tests
```

Each top-level `.rs` file in `tests/` is compiled as a separate test binary by Cargo. The `processors.rs` file acts as a module root that declares the `processors/` subdirectory modules:

```rust
mod common;
mod processors {
    pub mod cc_single_file;
    pub mod zspell;
    pub mod template;
}
```

This is the standard Rust pattern for grouping related integration tests into subdirectories without creating a separate binary per file.

## Shared helpers

`tests/common/mod.rs` provides utilities used across all test files:

| Helper | Purpose |
|---|---|
| `setup_test_project()` | Create an isolated project in a temp directory with `rsconstruct.toml` and basic directories |
| `setup_cc_project(path)` | Create a C project structure with the `cc_single_file` processor enabled |
| `run_rsconstruct(dir, args)` | Execute the `rsconstruct` binary in the given directory and return its output |
| `run_rsconstruct_with_env(dir, args, env)` | Same as `run_rsconstruct` but with extra environment variables (e.g., `NO_COLOR=1`) |

All helpers use `env!("CARGO_BIN_EXE_rsconstruct")` to locate the compiled binary, ensuring tests run against the freshly built version.

Every test creates a fresh `TempDir` for isolation. The directory is automatically cleaned up when the test ends.

## Test categories

### Command tests

Tests in `build.rs`, `clean`, `dry_run.rs`, `init.rs`, `status.rs`, and `watch.rs` exercise CLI commands end-to-end:

```rust
#[test]
fn force_rebuild() {
    let temp_dir = setup_test_project();
    // ... set up files ...
    let output = run_rsconstruct_with_env(temp_dir.path(), &["build", "--force"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("[template] Processing:"));
}
```

These tests verify exit codes, stdout messages, and side effects (files created or removed).

### Processor tests

Tests under `processors/` verify individual processor behavior: file discovery, compilation, linting, incremental skip logic, and error handling. Each processor test module follows the same pattern:

1. Set up a temp project with appropriate source files
2. Run `rsconstruct build`
3. Assert outputs exist and contain expected content
4. Optionally modify a file and rebuild to test incrementality

### Ignore tests

`rsconstructignore.rs` tests `.rsconstructignore` pattern matching: exact file patterns, glob patterns, leading `/` (anchored), trailing `/` (directory), comments, blank lines, and interaction with multiple processors.

## Common assertion patterns

**Exit code:**

```rust
assert!(output.status.success());
```

**Stdout content:**

```rust
let stdout = String::from_utf8_lossy(&output.stdout);
assert!(stdout.contains("Processing:"));
assert!(!stdout.contains("error"));
```

**File existence:**

```rust
assert!(path.join("out/cc_single_file/main.elf").exists());
```

**Incremental builds:**

```rust
// First build
run_rsconstruct(path, &["build"]);

// Second build should skip
let output = run_rsconstruct_with_env(path, &["build"], &[("NO_COLOR", "1")]);
let stdout = String::from_utf8_lossy(&output.stdout);
assert!(stdout.contains("Skipping (unchanged):"));
```

**Mtime-dependent rebuilds:**

```rust
// Modify a file and wait for mtime to differ
std::thread::sleep(std::time::Duration::from_millis(100));
fs::write(path.join("src/header.h"), "// changed\n").unwrap();

let output = run_rsconstruct(path, &["build"]);
let stdout = String::from_utf8_lossy(&output.stdout);
assert!(stdout.contains("Processing:"));
```

## Writing a new test

1. Add a test function in the appropriate file (or create a new `.rs` file under `tests/` for a new feature area)
2. Use `setup_test_project()` or `setup_cc_project()` to create an isolated environment
3. Write source files and configuration into the temp directory
4. Run `rsconstruct` with `run_rsconstruct()` or `run_rsconstruct_with_env()`
5. Assert on exit code, stdout/stderr content, and output file existence

If adding a new processor test module, declare it in `tests/processors.rs`:

```rust
mod processors {
    pub mod cc_single_file;
    pub mod zspell;
    pub mod template;
    pub mod my_new_processor;  // add here
}
```

## Test coverage by area

| Area | File | Tests |
|---|---|---|
| Build command | `build.rs` | Force rebuild, incremental skip, clean, deterministic order, keep-going, timings, parallel -j flag, parallel keep-going, parallel all-products, parallel timings, parallel caching |
| Cache | `cache.rs` | Clear, size, trim, list operations |
| Complete | `complete.rs` | Bash/zsh/fish generation, config-driven completion |
| Config | `config.rs` | Show merged config, show defaults, annotation comments |
| Dry run | `dry_run.rs` | Preview output, force flag, short flag |
| Graph | `graph.rs` | DOT, mermaid, JSON, text formats, empty project |
| Init | `init.rs` | Project creation, duplicate detection, existing directory preservation |
| Processor command | `processor_cmd.rs` | List, all, auto-detect, files, unknown processor error |
| Status | `status.rs` | UP-TO-DATE / STALE / RESTORABLE reporting |
| Tools | `tools.rs` | List tools, list all, check availability |
| Watch | `watch.rs` | Initial build, rebuild on change |
| Ignore | `rsconstructignore.rs` | Exact match, globs, leading slash, trailing slash, comments, cross-processor |
| Template | `processors/template.rs` | Rendering, incremental, dep_inputs |
| CC | `processors/cc_single_file.rs` | Compilation, headers, per-file flags, mixed C/C++, config change detection |
| Zspell | `processors/zspell.rs` | Correct/misspelled words, code block filtering, custom words, incremental |
