# Yamllint Processor

## Purpose

Lints YAML files using [yamllint](https://github.com/adrienverge/yamllint).

## How It Works

Discovers `.yml` and `.yaml` files in the project (excluding common build tool
directories), runs `yamllint` on each file, and records success in the cache.
A non-zero exit code from yamllint fails the product.

This processor supports batch mode, allowing multiple files to be checked in a
single yamllint invocation for better performance.

## Source Files

- Input: `**/*.yml`, `**/*.yaml`
- Output: none (checker)

## Configuration

```toml
[processor.yamllint]
command = "yamllint"                          # The yamllint command to run
args = []                                    # Additional arguments to pass to yamllint
dep_inputs = []                            # Additional files that trigger rebuilds when changed
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `command` | string | `"yamllint"` | The yamllint executable to run |
| `args` | string[] | `[]` | Extra arguments passed to yamllint |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

The tool accepts multiple files on the command line. When batching is enabled (default), rsconstruct passes all files in a single invocation for better performance.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../processors.md#clean-behavior) and [`rsconstruct clean`](../commands.md#rsconstruct-clean).
