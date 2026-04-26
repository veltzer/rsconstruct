# Mypy Processor

## Purpose

Type-checks Python source files using [mypy](https://mypy.readthedocs.io/).

## How It Works

Discovers `.py` files in the project (excluding common non-source directories),
runs `mypy` on each file, and creates a stub file on success.
A non-zero exit code from mypy fails the product.

This processor supports batch mode, allowing multiple files to be checked in a
single mypy invocation for better performance.

If a `mypy.ini` file exists in the project root, it is automatically added as an
extra input so that configuration changes trigger rebuilds.

## Source Files

- Input: `**/*.py`
- Output: `out/mypy/{flat_name}.mypy`

## Configuration

```toml
[processor.mypy]
command = "mypy"                             # The mypy command to run
args = []                                    # Additional arguments to pass to mypy
dep_inputs = []                            # Additional files that trigger rebuilds (e.g. ["pyproject.toml"])
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `command` | string | `"mypy"` | The mypy executable to run |
| `args` | string[] | `[]` | Extra arguments passed to mypy |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

The tool accepts multiple files on the command line. When batching is enabled (default), rsconstruct passes all files in a single invocation for better performance.

## Using mypy.ini

Mypy automatically reads configuration from a `mypy.ini` file in the project
root. This file is detected automatically and added as an extra input, so
changes to it will trigger rebuilds without manual configuration.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../processors.md#clean-behavior) and [`rsconstruct clean`](../commands.md#rsconstruct-clean).
