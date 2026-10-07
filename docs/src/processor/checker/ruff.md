# Ruff Processor

## Purpose

Lints Python source files using [ruff](https://docs.astral.sh/ruff/).

## How It Works

Discovers `.py` files in the project (excluding common non-source directories),
runs `ruff check` on each file, and creates a stub file on success.
A non-zero exit code from ruff fails the product.

This processor supports batch mode, allowing multiple files to be checked in a
single ruff invocation for better performance.

## Source Files

- Input: `**/*.py`
- Output: none (checker — a passing check is recorded in the cache, no file is written)

## Configuration

```toml
[processor.checker.ruff]
command = "ruff"                            # The ruff command to run
args = []                                  # Additional arguments to pass to ruff
dep_inputs = []                          # Additional files that trigger rebuilds (e.g. ["pyproject.toml"])
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `command` | string | `"ruff"` | The ruff executable to run |
| `args` | string[] | `[]` | Extra arguments passed to ruff |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |
| `output_depends_on_input_name` | bool | `true` | Key cached results by file path as well as content, because `per-file-ignores`, `INP001`, `N999` and import sorting depend on where a file lives. A renamed file is re-checked instead of reusing its old verdict. See [`output_depends_on_input_name`](../../configuration.md) |

## Batch support

The tool accepts multiple files on the command line. When batching is enabled (default), rsconstruct passes all files in a single invocation for better performance.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
