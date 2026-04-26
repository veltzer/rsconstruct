# Ascii Check Processor

## Purpose

Validates that files contain only ASCII characters.

## How It Works

Discovers `.md` files in the project and checks each for non-ASCII characters.
Files containing non-ASCII bytes fail the check. This is a built-in processor
that does not require any external tools.

This processor supports batch mode, allowing multiple files to be checked in a
single invocation.

## Source Files

- Input: `**/*.md`
- Output: none (checker)

## Configuration

```toml
[processor.ascii]
args = []                              # Additional arguments (unused, for consistency)
dep_inputs = []                      # Additional files that trigger rebuilds when changed
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `args` | string[] | `[]` | Extra arguments (reserved for future use) |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

The tool accepts multiple files on the command line. When batching is enabled (default), rsconstruct passes all files in a single invocation for better performance.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../processors.md#clean-behavior) and [`rsconstruct clean`](../commands.md#rsconstruct-clean).
