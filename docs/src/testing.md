# Testing

RSB uses integration tests exclusively. All tests live in the `tests/` directory and exercise the compiled `rsb` binary as a black box — no unit tests are embedded in `src/`.

## Running tests

```bash
cargo test              # Run all tests
cargo test rsbignore    # Run tests matching a name
cargo test -- --nocapture  # Show stdout/stderr from tests
```

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
├── rsbignore.rs                # .rsbignore / .gitignore exclusion tests
├── status.rs                   # Status command tests
├── tools.rs                    # Tools list/check tests
├── watch.rs                    # File watcher tests
├── processors.rs               # Module root for processor tests
└── processors/
    ├── cc_single_file.rs       # C/C++ compilation tests
    ├── sleep.rs                # Sleep processor tests
    ├── spellcheck.rs           # Spellcheck processor tests
    └── template.rs             # Template processor tests
```

Each top-level `.rs` file in `tests/` is compiled as a separate test binary by Cargo. The `processors.rs` file acts as a module root that declares the `processors/` subdirectory modules:

```rust
mod common;
mod processors {
    pub mod cc_single_file;
    pub mod sleep;
    pub mod spellcheck;
    pub mod template;
}
```

This is the standard Rust pattern for grouping related integration tests into subdirectories without creating a separate binary per file.

## Shared helpers

`tests/common/mod.rs` provides utilities used across all test files:

| Helper | Purpose |
|---|---|
| `setup_test_project()` | Create an isolated project in a temp directory with `rsb.toml` and basic directories |
| `setup_cc_project(path)` | Create a C project structure with the `cc_single_file` processor enabled |
| `run_rsb(dir, args)` | Execute the `rsb` binary in the given directory and return its output |
| `run_rsb_with_env(dir, args, env)` | Same as `run_rsb` but with extra environment variables (e.g., `NO_COLOR=1`) |

All helpers use `env!("CARGO_BIN_EXE_rsb")` to locate the compiled binary, ensuring tests run against the freshly built version.

Every test creates a fresh `TempDir` for isolation. The directory is automatically cleaned up when the test ends.

## Test categories

### Command tests

Tests in `build.rs`, `clean`, `dry_run.rs`, `init.rs`, `status.rs`, and `watch.rs` exercise CLI commands end-to-end:

```rust
#[test]
fn force_rebuild() {
    let temp_dir = setup_test_project();
    // ... set up files ...
    let output = run_rsb_with_env(temp_dir.path(), &["build", "--force"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("[template] Processing:"));
}
```

These tests verify exit codes, stdout messages, and side effects (files created or removed).

### Processor tests

Tests under `processors/` verify individual processor behavior: file discovery, compilation, linting, incremental skip logic, and error handling. Each processor test module follows the same pattern:

1. Set up a temp project with appropriate source files
2. Run `rsb build`
3. Assert outputs exist and contain expected content
4. Optionally modify a file and rebuild to test incrementality

### Ignore tests

`rsbignore.rs` tests `.rsbignore` pattern matching: exact file patterns, glob patterns, leading `/` (anchored), trailing `/` (directory), comments, blank lines, and interaction with multiple processors.

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
assert!(!path.join("out/sleep/excluded.done").exists());
```

**Incremental builds:**

```rust
// First build
run_rsb(path, &["build"]);

// Second build should skip
let output = run_rsb_with_env(path, &["build"], &[("NO_COLOR", "1")]);
let stdout = String::from_utf8_lossy(&output.stdout);
assert!(stdout.contains("Skipping (unchanged):"));
```

**Mtime-dependent rebuilds:**

```rust
// Modify a file and wait for mtime to differ
std::thread::sleep(std::time::Duration::from_millis(100));
fs::write(path.join("src/header.h"), "// changed\n").unwrap();

let output = run_rsb(path, &["build"]);
let stdout = String::from_utf8_lossy(&output.stdout);
assert!(stdout.contains("Processing:"));
```

## Writing a new test

1. Add a test function in the appropriate file (or create a new `.rs` file under `tests/` for a new feature area)
2. Use `setup_test_project()` or `setup_cc_project()` to create an isolated environment
3. Write source files and configuration into the temp directory
4. Run `rsb` with `run_rsb()` or `run_rsb_with_env()`
5. Assert on exit code, stdout/stderr content, and output file existence

If adding a new processor test module, declare it in `tests/processors.rs`:

```rust
mod processors {
    pub mod cc_single_file;
    pub mod sleep;
    pub mod spellcheck;
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
| Ignore | `rsbignore.rs` | Exact match, globs, leading slash, trailing slash, comments, cross-processor |
| Template | `processors/template.rs` | Rendering, incremental, extra_inputs |
| CC | `processors/cc_single_file.rs` | Compilation, headers, per-file flags, mixed C/C++, config change detection |
| Sleep | `processors/sleep.rs` | Basic execution, extra_inputs |
| Spellcheck | `processors/spellcheck.rs` | Correct/misspelled words, code block filtering, custom words, incremental |
