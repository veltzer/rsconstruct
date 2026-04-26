# Sphinx Processor

## Purpose

Builds Sphinx documentation projects.

## How It Works

Discovers `conf.py` files indicating Sphinx projects, collects sibling `.rst`,
`.py`, and `.md` files as inputs, and runs `sphinx-build` to generate output.
A non-zero exit code fails the product.

## Source Files

- Input: `**/conf.py` (plus sibling `.rst`, `.py`, `.md` files)
- Output: none (creator — produces output in `_build` directory)

## Configuration

```toml
[processor.sphinx]
command = "sphinx-build"               # The sphinx-build command to run
output_dir = "_build"                  # Output directory for generated docs
args = []                              # Additional arguments to pass to sphinx-build
dep_inputs = []                      # Additional files that trigger rebuilds when changed
cache_output_dir = true                # Cache the output directory for fast restore after clean
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `command` | string | `"sphinx-build"` | The sphinx-build executable to run |
| `output_dir` | string | `"_build"` | Output directory for generated documentation |
| `args` | string[] | `[]` | Extra arguments passed to sphinx-build |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |
| `cache_output_dir` | boolean | `true` | Cache the `_build/` directory so `rsconstruct clean && rsconstruct build` restores from cache |

## Batch support

Runs as a single whole-project operation (e.g., `cargo build`, `npm install`).

## Clean behavior

This processor is a Creator — `rsconstruct clean outputs` removes its declared `output_dirs` recursively (the build tool produces an unknown set of files inside, so directory-level deletion is the only option). After all per-product cleans complete, the orchestrator removes any parent directories that are now empty. Pass `--no-empty-dirs` to keep them. See [Clean behavior](../processors.md#clean-behavior) and [`rsconstruct clean`](../commands.md#rsconstruct-clean).
