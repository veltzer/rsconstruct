# Script Processor

## Purpose

Runs a user-configured script or command as a linter on discovered files. This
is a generic linter that lets you plug in any script without writing a custom
processor.

## How It Works

Discovers files matching the configured extensions in the configured scan
directory, then runs the configured linter command on each file (or batch of
files). A non-zero exit code from the script fails the product.

The `command` field has no default: declaring the processor without one is an
error when it runs.

This processor supports batch mode, allowing multiple files to be checked in a
single invocation for better performance.

## Source Files

- Input: configured via `src_extensions` and `src_dirs`
- Output: none (checker)

## Configuration

```toml
[processor.checker.script]
command = "python"
args = ["scripts/md_lint.py", "-q"]
src_extensions = [".md"]
src_dirs = ["marp"]
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `enabled` | bool | `true` | Set to `false` to keep the stanza but skip the processor |
| `command` | string | (required) | The command to run |
| `args` | string[] | `[]` | Extra arguments passed before file paths |
| `src_extensions` | string[] | `[]` | File extensions to scan for |
| `src_dirs` | string[] | `[]` | Directories to scan (`[""]` for the whole project) |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |
| `dep_auto` | string[] | `[]` | Auto-detected input files |

## Batch support

The tool accepts multiple files on the command line. When batching is enabled (default), rsconstruct passes all files in a single invocation for better performance.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
