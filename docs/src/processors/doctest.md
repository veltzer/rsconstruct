# Doctest Processor

## Purpose

Runs Python doctests embedded in `.py` files using `python3 -m doctest`.

## How It Works

Python files (`.py`) are checked for embedded doctests. Each file is run through
`python3 -m doctest` — failing doctests cause the build to fail.

## Source Files

- Input: `**/*.py`
- Output: none (checker — pass/fail only)

## Configuration

```toml
[processor.doctest]
src_extensions = [".py"]                      # File extensions to process (default: [".py"])
dep_inputs = []                         # Additional files that trigger rebuilds
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `src_extensions` | string[] | `[".py"]` | File extensions to discover |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

The tool accepts multiple files on the command line. When batching is enabled (default), rsconstruct passes all files in a single invocation for better performance.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../processors.md#clean-behavior) and [`rsconstruct clean`](../commands.md#rsconstruct-clean).
