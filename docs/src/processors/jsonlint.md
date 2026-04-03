# Jsonlint Processor

## Purpose

Lints JSON files using [jsonlint](https://github.com/zaach/jsonlint).

## How It Works

Discovers `.json` files in the project (excluding common build tool
directories), runs `jsonlint` on each file, and records success in the cache.
A non-zero exit code from jsonlint fails the product.

This processor does not support batch mode — each file is checked individually.

## Source Files

- Input: `**/*.json`
- Output: none (checker)

## Configuration

```toml
[processor.jsonlint]
linter = "jsonlint"                          # The jsonlint command to run
args = []                                    # Additional arguments to pass to jsonlint
extra_inputs = []                            # Additional files that trigger rebuilds when changed
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `linter` | string | `"jsonlint"` | The jsonlint executable to run |
| `args` | string[] | `[]` | Extra arguments passed to jsonlint |
| `extra_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

The tool accepts multiple files on the command line. When batching is enabled (default), rsconstruct passes all files in a single invocation for better performance.
