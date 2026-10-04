# Markdownlint Processor

## Purpose

Lints Markdown files using [markdownlint](https://github.com/DavidAnson/markdownlint) (Node.js).

## How It Works

Discovers `.md` files in the project and runs `markdownlint` on each file. A
non-zero exit code fails the product.

Depends on the npm processor — uses the `markdownlint` binary installed by npm.

## Source Files

- Input: `**/*.md`
- Output: none (checker)

## Configuration

```toml
[processor.checker.markdownlint]
command = "markdownlint"               # Path to the markdownlint binary
args = []                              # Additional arguments to pass to markdownlint
dep_inputs = []                      # Additional files that trigger rebuilds when changed
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `command` | string | `"markdownlint"` | Path to the markdownlint executable |
| `args` | string[] | `[]` | Extra arguments passed to markdownlint |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

The tool processes one file at a time. Each file is checked in a separate invocation.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
