# Rumdl Processor

## Purpose

Lints Markdown files using [rumdl](https://github.com/veltzer/rumdl).

## How It Works

Discovers `.md` files in the project (excluding common non-source directories),
runs `rumdl check` on each file, and creates a stub file on success.
A non-zero exit code from rumdl fails the product.

This processor supports batch mode, allowing multiple files to be checked in a
single rumdl invocation for better performance.

Every configuration file rumdl's discovery reads from the working directory is
an input of every product when it exists (`dep_auto`): its own `.rumdl.toml`,
`rumdl.toml`, `.config/rumdl.toml` and `pyproject.toml` (`[tool.rumdl]`), and
— when none of those is present — the markdownlint configuration it falls back
to (`.markdownlint-cli2.jsonc`/`.yaml`/`.yml`, `.markdownlint.json`/`.jsonc`/
`.yaml`/`.yml`, `markdownlint.json`/`.jsonc`/`.yaml`). An edit re-checks every
file, with no `dep_inputs` entry needed. A file named with `--config` in `args`
is not tracked; list it in `dep_inputs`.

## Source Files

- Input: `**/*.md`
- Output: none (checker — a passing check is recorded in the cache, no file is written)

## Configuration

```toml
[processor.checker.rumdl]
command = "rumdl"                             # The rumdl command to run
args = []                                    # Additional arguments to pass to rumdl
dep_inputs = []                            # Additional files that trigger rebuilds when changed
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `command` | string | `"rumdl"` | The rumdl executable to run |
| `args` | string[] | `[]` | Extra arguments passed to rumdl |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |
| `dep_auto` | string[] | (the list above) | Config files that are inputs when they exist |

## Batch support

The tool accepts multiple files on the command line. When batching is enabled (default), rsconstruct passes all files in a single invocation for better performance.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
