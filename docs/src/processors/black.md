# Black Processor

## Purpose

Checks Python file formatting using [Black](https://black.readthedocs.io/), the uncompromising code formatter. Runs `black --check` which verifies files are already formatted without modifying them.

## How It Works

Python files matching configured extensions are checked via `black --check`. The command exits with a non-zero status if any file would be reformatted, causing the build to fail.

## Source Files

- Input: `**/*{src_extensions}` (default: `*.py`)

## Configuration

```toml
[processor.black]
src_extensions = [".py"]                      # File extensions to check (default: [".py"])
dep_inputs = []                         # Additional files that trigger rechecks when changed
args = []                                 # Extra arguments passed to black
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `src_extensions` | string[] | `[".py"]` | File extensions to discover |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rechecks |
| `dep_auto` | string[] | `["pyproject.toml"]` | Config files that auto-trigger rechecks |
| `args` | string[] | `[]` | Additional arguments passed to `black` |

## Batch support

The tool accepts multiple files on the command line. When batching is enabled (default), rsconstruct passes all files in a single invocation for better performance.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../processors.md#clean-behavior) and [`rsconstruct clean`](../commands.md#rsconstruct-clean).
