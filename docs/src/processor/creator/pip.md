# Pip Processor

## Purpose

Installs Python dependencies from `requirements.txt` files using pip.

## How It Works

Discovers `requirements.txt` files in the project and runs `pip install -r`
in each file's directory. A successful install is recorded in the cache, so
dependencies are only reinstalled when `requirements.txt` (or a `dep_inputs`
file) changes. No file is written and the installed packages are not cached:
pip installs into whatever environment is active, which is not a directory
rsconstruct owns.

## Source Files

- Input: `**/requirements.txt`
- Output: none (the install itself; nothing is cached or restored)

## Configuration

```toml
[processor.creator.pip]
command = "pip"                        # The pip command to run
args = []                              # Additional arguments to pass to pip
dep_inputs = []                      # Additional files that trigger rebuilds when changed
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `command` | string | `"pip"` | The pip executable to run |
| `args` | string[] | `[]` | Extra arguments passed to pip |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

Runs as a single whole-project operation (e.g., `cargo build`, `npm install`).

## Clean behavior

This processor is a Creator with no declared outputs — `rsconstruct clean outputs` removes nothing for it; installed packages stay installed. See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
