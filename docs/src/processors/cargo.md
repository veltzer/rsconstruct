# Cargo Processor

## Purpose

Builds Rust projects using Cargo. Each `Cargo.toml` produces a cached success
marker, allowing RSConstruct to skip rebuilds when source files haven't changed.

## How It Works

Discovers files named `Cargo.toml` in the project. For each Cargo.toml found,
the processor runs `cargo build` (or a configured command) in that directory.

### Input Tracking

The cargo processor tracks **all `.rs` and `.toml` files** in the Cargo.toml's
directory tree as inputs. This includes:

- `Cargo.toml` and `Cargo.lock`
- All Rust source files (`src/**/*.rs`)
- Test files, examples, benches
- Workspace member Cargo.toml files

When any tracked file changes, rsconstruct will re-run cargo.

### Workspaces

For Cargo workspaces, each `Cargo.toml` (root and members) is discovered as a
separate product. To build only the workspace root, use `src_exclude_paths` to skip
member directories, or configure `src_dirs` to limit discovery.

## Source Files

- Input: `Cargo.toml` plus all `.rs` and `.toml` files in the project tree
- Output: None (creator — produces output in `target` directory)

## Configuration

```toml
[processor.cargo]
cargo = "cargo"          # Cargo binary to use
command = "build"        # Cargo command (build, check, test, clippy, etc.)
args = []                # Extra arguments passed to cargo
profiles = ["dev", "release"]  # Cargo profiles to build
src_dirs = [""]            # Directory to scan ("" = project root)
src_extensions = ["Cargo.toml"]
dep_inputs = []        # Additional files that trigger rebuilds
cache_output_dir = true  # Cache the target/ directory for fast restore after clean
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `cargo` | string | `"cargo"` | Path or name of the cargo binary |
| `command` | string | `"build"` | Cargo subcommand to run |
| `args` | string[] | `[]` | Extra arguments passed to cargo |
| `profiles` | string[] | `["dev", "release"]` | Cargo profiles to build (creates one product per profile) |
| `src_dirs` | string[] | `[""]` | Directory to scan for Cargo.toml files |
| `src_extensions` | string[] | `["Cargo.toml"]` | File names to match |
| `src_exclude_dirs` | string[] | `["/.git/", "/target/", ...]` | Directory patterns to exclude |
| `src_exclude_paths` | string[] | `[]` | Paths (relative to project root) to exclude |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |
| `cache_output_dir` | boolean | `true` | Cache the `target/` directory so `rsconstruct clean && rsconstruct build` restores from cache. Consider disabling for large projects. |

## Batch support

Runs as a single whole-project operation (e.g., `cargo build`, `npm install`).

## Examples

### Basic Usage

```toml
[processor.cargo]
```

### Release Only

```toml
[processor.cargo]
profiles = ["release"]
```

### Dev Only

```toml
[processor.cargo]
profiles = ["dev"]
```

### Use cargo check Instead of build

```toml
[processor.cargo]
command = "check"
```

### Run clippy

```toml
[processor.cargo]
command = "clippy"
args = ["--", "-D", "warnings"]
```

### Workspace Root Only

```toml
[processor.cargo]
src_exclude_paths = ["crates/"]
```

## Notes

- Cargo has its own incremental compilation, so rsconstruct's caching mainly avoids
  invoking cargo at all when nothing changed
- The `target/` directory is automatically excluded from input scanning
- For monorepos with multiple Rust projects, each Cargo.toml is built separately
