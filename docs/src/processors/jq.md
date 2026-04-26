# Jq Processor

## Purpose

Validates JSON files using [jq](https://jqlang.org/).

## How It Works

Discovers `.json` files in the project (excluding common build tool
directories), runs `jq empty` on each file, and records success in the cache.
The `empty` filter validates JSON syntax without producing output — a non-zero
exit code from jq fails the product.

This processor supports batch mode — multiple files are checked in a single
jq invocation.

## Source Files

- Input: `**/*.json`
- Output: none (linter)

## Configuration

```toml
[processor.jq]
command = "jq"                               # The jq command to run
args = []                                    # Additional arguments to pass to jq (after "empty")
dep_inputs = []                            # Additional files that trigger rebuilds when changed
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `command` | string | `"jq"` | The jq executable to run |
| `args` | string[] | `[]` | Extra arguments passed to jq (after the `empty` filter) |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

The tool accepts multiple files on the command line. When batching is enabled (default), rsconstruct passes all files in a single invocation for better performance.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../processors.md#clean-behavior) and [`rsconstruct clean`](../commands.md#rsconstruct-clean).
