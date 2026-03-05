# Clippy Processor

## Purpose

Lints Rust projects using [Cargo Clippy](https://doc.rust-lang.org/clippy/). Each `Cargo.toml`
produces a cached success marker, allowing RSBuild to skip re-linting when source files haven't changed.

## How It Works

Discovers files named `Cargo.toml` in the project. For each Cargo.toml found,
the processor runs `cargo clippy` in that directory. A non-zero exit code fails the product.

### Input Tracking

The clippy processor tracks **all `.rs` and `.toml` files** in the Cargo.toml's
directory tree as inputs. This includes:

- `Cargo.toml` and `Cargo.lock`
- All Rust source files (`src/**/*.rs`)
- Test files, examples, benches
- Workspace member Cargo.toml files

When any tracked file changes, rsbuild will re-run clippy.

## Source Files

- Input: `Cargo.toml` plus all `.rs` and `.toml` files in the project tree
- Output: None (checker-style caching)

## Configuration

```toml
[processor]
enabled = ["clippy"]

[processor.clippy]
cargo = "cargo"          # Cargo binary to use
command = "clippy"       # Cargo command (usually "clippy")
args = []                # Extra arguments passed to cargo clippy
scan_dir = ""            # Directory to scan ("" = project root)
extensions = ["Cargo.toml"]
extra_inputs = []        # Additional files that trigger rebuilds
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `cargo` | string | `"cargo"` | Path or name of the cargo binary |
| `command` | string | `"clippy"` | Cargo subcommand to run |
| `args` | string[] | `[]` | Extra arguments passed to cargo clippy |
| `scan_dir` | string | `""` | Directory to scan for Cargo.toml files |
| `extensions` | string[] | `["Cargo.toml"]` | File names to match |
| `exclude_dirs` | string[] | `["/.git/", "/target/", ...]` | Directory patterns to exclude |
| `exclude_paths` | string[] | `[]` | Paths (relative to project root) to exclude |
| `extra_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Examples

### Basic Usage

```toml
[processor]
enabled = ["clippy"]
```

### Deny All Warnings

```toml
[processor]
enabled = ["clippy"]

[processor.clippy]
args = ["--", "-D", "warnings"]
```

### Use Both Cargo Build and Clippy

```toml
[processor]
enabled = ["cargo", "clippy"]
```

## Notes

- Clippy uses the `cargo` binary which is shared with the cargo processor
- The `target/` directory is automatically excluded from input scanning
- For monorepos with multiple Rust projects, each Cargo.toml is linted separately
