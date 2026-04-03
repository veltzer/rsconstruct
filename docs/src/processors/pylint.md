# Pylint Processor

## Purpose

Lints Python source files using [pylint](https://pylint.readthedocs.io/).

## How It Works

Discovers `.py` files in the project (excluding common non-source directories),
runs `pylint` on each file, and creates a stub file on success.
A non-zero exit code from pylint fails the product.

This processor supports batch mode, allowing multiple files to be checked in a
single pylint invocation for better performance.

If a `.pylintrc` file exists in the project root, it is automatically added as an
extra input so that configuration changes trigger rebuilds.

## Source Files

- Input: `**/*.py`
- Output: `out/pylint/{flat_name}.pylint`

## Configuration

```toml
[processor.pylint]
args = []                                  # Additional arguments to pass to pylint
extra_inputs = []                          # Additional files that trigger rebuilds (e.g. ["pyproject.toml"])
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `args` | string[] | `[]` | Extra arguments passed to pylint |
| `extra_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

The tool accepts multiple files on the command line. When batching is enabled (default), rsconstruct passes all files in a single invocation for better performance.
