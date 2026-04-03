# Taplo Processor

## Purpose

Checks TOML files using [taplo](https://taplo.tamasfe.dev/).

## How It Works

Discovers `.toml` files in the project (excluding common build tool
directories), runs `taplo check` on each file, and records success in the cache.
A non-zero exit code from taplo fails the product.

This processor supports batch mode, allowing multiple files to be checked in a
single taplo invocation for better performance.

## Source Files

- Input: `**/*.toml`
- Output: none (checker)

## Configuration

```toml
[processor.taplo]
linter = "taplo"                             # The taplo command to run
args = []                                    # Additional arguments to pass to taplo
extra_inputs = []                            # Additional files that trigger rebuilds when changed
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `linter` | string | `"taplo"` | The taplo executable to run |
| `args` | string[] | `[]` | Extra arguments passed to taplo |
| `extra_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

The tool accepts multiple files on the command line. When batching is enabled (default), rsconstruct passes all files in a single invocation for better performance.
