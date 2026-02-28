# Cargo Processor

## Purpose

Builds Rust projects using Cargo. Each `Cargo.toml` produces a cached success
marker, allowing RSB to skip rebuilds when source files haven't changed.

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

When any tracked file changes, rsb will re-run cargo.

### Workspaces

For Cargo workspaces, each `Cargo.toml` (root and members) is discovered as a
separate product. To build only the workspace root, use `exclude_paths` to skip
member directories, or configure `scan_dir` to limit discovery.

## Source Files

- Input: `Cargo.toml` plus all `.rs` and `.toml` files in the project tree
- Output: None (mass_generator — produces output in `target` directory)

## Configuration

```toml
[processor]
enabled = ["cargo"]

[processor.cargo]
cargo = "cargo"          # Cargo binary to use
command = "build"        # Cargo command (build, check, test, clippy, etc.)
args = []                # Extra arguments passed to cargo
scan_dir = ""            # Directory to scan ("" = project root)
extensions = ["Cargo.toml"]
extra_inputs = []        # Additional files that trigger rebuilds
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `cargo` | string | `"cargo"` | Path or name of the cargo binary |
| `command` | string | `"build"` | Cargo subcommand to run |
| `args` | string[] | `[]` | Extra arguments passed to cargo |
| `scan_dir` | string | `""` | Directory to scan for Cargo.toml files |
| `extensions` | string[] | `["Cargo.toml"]` | File names to match |
| `exclude_dirs` | string[] | `["/.git/", "/target/", ...]` | Directory patterns to exclude |
| `exclude_paths` | string[] | `[]` | Paths (relative to project root) to exclude |
| `extra_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Examples

### Basic Usage

```toml
[processor]
enabled = ["cargo"]
```

### Release Build

```toml
[processor]
enabled = ["cargo"]

[processor.cargo]
args = ["--release"]
```

### Use cargo check Instead of build

```toml
[processor]
enabled = ["cargo"]

[processor.cargo]
command = "check"
```

### Run clippy

```toml
[processor]
enabled = ["cargo"]

[processor.cargo]
command = "clippy"
args = ["--", "-D", "warnings"]
```

### Workspace Root Only

```toml
[processor]
enabled = ["cargo"]

[processor.cargo]
exclude_paths = ["crates/"]
```

## Notes

- Cargo has its own incremental compilation, so rsb's caching mainly avoids
  invoking cargo at all when nothing changed
- The `target/` directory is automatically excluded from input scanning
- For monorepos with multiple Rust projects, each Cargo.toml is built separately
