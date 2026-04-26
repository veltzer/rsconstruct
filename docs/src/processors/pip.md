# Pip Processor

## Purpose

Installs Python dependencies from `requirements.txt` files using pip.

## How It Works

Discovers `requirements.txt` files in the project, runs `pip install -r` on
each, and creates a stamp file on success. The stamp file tracks the install
state so dependencies are only reinstalled when `requirements.txt` changes.

## Source Files

- Input: `**/requirements.txt`
- Output: `out/pip/{flat_name}.stamp`

## Configuration

```toml
[processor.pip]
command = "pip"                        # The pip command to run
args = []                              # Additional arguments to pass to pip
dep_inputs = []                      # Additional files that trigger rebuilds when changed
cache_output_dir = true                # Cache the stamp directory for fast restore after clean
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `command` | string | `"pip"` | The pip executable to run |
| `args` | string[] | `[]` | Extra arguments passed to pip |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |
| `cache_output_dir` | boolean | `true` | Cache the `out/pip/` directory so `rsconstruct clean && rsconstruct build` restores from cache |

## Batch support

Runs as a single whole-project operation (e.g., `cargo build`, `npm install`).

## Clean behavior

This processor is a Creator — `rsconstruct clean outputs` removes its declared `output_dirs` recursively (the build tool produces an unknown set of files inside, so directory-level deletion is the only option). After all per-product cleans complete, the orchestrator removes any parent directories that are now empty. Pass `--no-empty-dirs` to keep them. See [Clean behavior](../processors.md#clean-behavior) and [`rsconstruct clean`](../commands.md#rsconstruct-clean).
