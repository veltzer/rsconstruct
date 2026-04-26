# Shellcheck Processor

## Purpose

Lints shell scripts using [shellcheck](https://www.shellcheck.net/).

## How It Works

Discovers `.sh` and `.bash` files in the project (excluding common build tool
directories), runs `shellcheck` on each file, and records success in the cache.
A non-zero exit code from shellcheck fails the product.

This processor supports batch mode, allowing multiple files to be checked in a
single shellcheck invocation for better performance.

## Source Files

- Input: `**/*.sh`, `**/*.bash`
- Output: none (linter)

## Configuration

```toml
[processor.shellcheck]
command = "shellcheck"                       # The shellcheck command to run
args = []                                    # Additional arguments to pass to shellcheck
dep_inputs = []                            # Additional files that trigger rebuilds when changed
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `command` | string | `"shellcheck"` | The shellcheck executable to run |
| `args` | string[] | `[]` | Extra arguments passed to shellcheck |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

The tool accepts multiple files on the command line. When batching is enabled (default), rsconstruct passes all files in a single invocation for better performance.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../processors.md#clean-behavior) and [`rsconstruct clean`](../commands.md#rsconstruct-clean).
