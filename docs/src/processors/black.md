# Black Processor

## Purpose

Checks Python file formatting using [Black](https://black.readthedocs.io/), the uncompromising code formatter. Runs `black --check` which verifies files are already formatted without modifying them.

## How It Works

Python files matching configured extensions are checked via `black --check`. The command exits with a non-zero status if any file would be reformatted, causing the build to fail.

## Source Files

- Input: `**/*{extensions}` (default: `*.py`)

## Configuration

```toml
[processor.black]
extensions = [".py"]                      # File extensions to check (default: [".py"])
extra_inputs = []                         # Additional files that trigger rechecks when changed
args = []                                 # Extra arguments passed to black
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `extensions` | string[] | `[".py"]` | File extensions to discover |
| `extra_inputs` | string[] | `[]` | Extra files whose changes trigger rechecks |
| `auto_inputs` | string[] | `["pyproject.toml"]` | Config files that auto-trigger rechecks |
| `args` | string[] | `[]` | Additional arguments passed to `black` |

## Batch support

The tool accepts multiple files on the command line. When batching is enabled (default), rsconstruct passes all files in a single invocation for better performance.
