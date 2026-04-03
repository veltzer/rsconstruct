# Rumdl Processor

## Purpose

Lints Markdown files using [rumdl](https://github.com/veltzer/rumdl).

## How It Works

Discovers `.md` files in the project (excluding common non-source directories),
runs `rumdl check` on each file, and creates a stub file on success.
A non-zero exit code from rumdl fails the product.

This processor supports batch mode, allowing multiple files to be checked in a
single rumdl invocation for better performance.

## Source Files

- Input: `**/*.md`
- Output: `out/rumdl/{flat_name}.rumdl`

## Configuration

```toml
[processor.rumdl]
linter = "rumdl"                             # The rumdl command to run
args = []                                    # Additional arguments to pass to rumdl
extra_inputs = []                            # Additional files that trigger rebuilds when changed
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `linter` | string | `"rumdl"` | The rumdl executable to run |
| `args` | string[] | `[]` | Extra arguments passed to rumdl |
| `extra_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

The tool accepts multiple files on the command line. When batching is enabled (default), rsconstruct passes all files in a single invocation for better performance.
