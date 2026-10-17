# Cargo Processor

## Purpose

Builds Rust projects using Cargo. Each crate target artifact (`<bin>`, `lib<name>.rlib`, `lib<name>.so`/`.dylib`, `lib<name>.a`) under `target/<profile>/` is planned in advance as its own cached product, while leaving Cargo's `target/<profile>/deps` incremental directory intact.

## How It Works

Discovers files named `Cargo.toml` in the project. For each `Cargo.toml` found,
the processor parses the manifest (and workspace members) to plan the output
artifacts for each configured profile, and runs `cargo build` (or a configured
command) at most once per `(Cargo.toml, profile)` when any planned product is dirty.

### Input Tracking

The cargo processor tracks **all `.rs` and `.toml` files, plus `Cargo.lock`,** in the Cargo.toml's
directory tree as inputs. This includes:

- `Cargo.toml` and `Cargo.lock`
- All Rust source files (`src/**/*.rs`)
- Test files, examples, benches
- Workspace member Cargo.toml files

When any tracked file changes, rsconstruct will re-run cargo.

### Workspaces

For Cargo workspaces, each `Cargo.toml` (root and members) is discovered as a
separate anchor. To build only the workspace root, use `src_exclude_paths` to skip
member directories, or configure `src_dirs` / `src_files` to limit discovery.

## Source Files

- Input: `Cargo.toml` plus all `.rs` and `.toml` files and `Cargo.lock` in the project tree
- Output: Planned crate target artifacts in `target/<profile_dir>/`

## Configuration

```toml
[processor.mass_generator.cargo]
cargo = "cargo"          # Cargo binary to use
command = "build"        # Cargo command (build, check, etc.)
args = []                # Extra arguments passed to cargo
profiles = ["dev", "release"]  # Cargo profiles to build
src_dirs = [""]          # Directory to scan ("" = project root)
src_extensions = ["Cargo.toml"]
dep_inputs = []          # Additional files that trigger rebuilds
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `cargo` | string | `"cargo"` | Path or name of the cargo binary |
| `command` | string | `"build"` | Cargo subcommand to run |
| `args` | string[] | `[]` | Extra arguments passed to cargo |
| `profiles` | string[] | `["dev", "release"]` | Cargo profiles to build |
| `src_dirs` | string[] | `[""]` | Directory to scan for Cargo.toml files |
| `src_extensions` | string[] | `["Cargo.toml"]` | File names to match |
| `src_exclude_dirs` | string[] | `["/.git/", "/target/", ...]` | Directory patterns to exclude |
| `src_exclude_paths` | string[] | `[]` | Paths (relative to project root) to exclude |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

Runs once per `(Cargo.toml, profile)` when any planned artifact for that profile is dirty.

## Examples

### Basic Usage

```toml
[processor.mass_generator.cargo]
```

### Release Only

```toml
[processor.mass_generator.cargo]
profiles = ["release"]
```

### Dev Only

```toml
[processor.mass_generator.cargo]
profiles = ["dev"]
```

### Use cargo check Instead of build

```toml
[processor.mass_generator.cargo]
command = "check"
```

### Workspace Root Only

```toml
[processor.mass_generator.cargo]
src_exclude_paths = ["crates/"]
```

## Notes

- Each top-level crate artifact in `target/<profile_dir>/` is cached as a blob in the object store, while `target/<profile_dir>/deps` stays untouched so Cargo retains its own incremental compilation state
- The `target/` directory is automatically excluded from input scanning
- For monorepos with multiple Rust projects, each Cargo.toml is built separately

## Clean behavior

This processor is a Mass Generator — `rsconstruct clean outputs` removes each planned output file (`product.outputs`) and then sweeps any parent directories that are now empty. Pass `--no-empty-dirs` to keep them. See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
