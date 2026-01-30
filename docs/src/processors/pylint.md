# Pylint Processor

## Purpose

Lints Python source files using an external linter (ruff by default).

## How It Works

Discovers `.py` files in the project (excluding common non-source directories),
runs the configured linter on each file, and creates a stub file on success.
A non-zero exit code from the linter fails the product.

## Source Files

- Input: `**/*.py`
- Output: `out/pylint/{flat_name}.pylint`

## Configuration

```toml
[processor.pylint]
linter = "ruff"                            # Python linter command (default: "ruff")
args = []                                  # Additional arguments to pass to the linter
extra_inputs = ["pyproject.toml"]          # Additional files that trigger rebuilds when changed
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `linter` | string | `"ruff"` | The linter executable to invoke |
| `args` | string[] | `[]` | Extra arguments passed to the linter |
| `extra_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |
