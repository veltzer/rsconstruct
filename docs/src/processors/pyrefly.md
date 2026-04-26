# Pyrefly Processor

## Purpose

Type-checks Python source files using [pyrefly](https://pyrefly.org/).

## How It Works

Discovers `.py` files in the project (excluding common non-source directories),
runs `pyrefly check` on each file, and records success in the cache.
A non-zero exit code from pyrefly fails the product.

This processor supports batch mode, allowing multiple files to be checked in a
single pyrefly invocation for better performance.

## Source Files

- Input: `**/*.py`
- Output: none (linter)

## Configuration

```toml
[processor.pyrefly]
command = "pyrefly"                          # The pyrefly command to run
args = []                                    # Additional arguments to pass to pyrefly
dep_inputs = []                            # Additional files that trigger rebuilds when changed
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `command` | string | `"pyrefly"` | The pyrefly executable to run |
| `args` | string[] | `[]` | Extra arguments passed to pyrefly |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

The tool accepts multiple files on the command line. When batching is enabled (default), rsconstruct passes all files in a single invocation for better performance.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../processors.md#clean-behavior) and [`rsconstruct clean`](../commands.md#rsconstruct-clean).
