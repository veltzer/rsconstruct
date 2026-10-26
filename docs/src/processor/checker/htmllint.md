# HTMLLint Processor

## Purpose

Lints HTML files using [htmllint](https://github.com/htmllint/htmllint).

## How It Works

Discovers `.html` and `.htm` files in the project (excluding common build tool
directories), runs `htmllint` on each file, and records success in the cache.
A non-zero exit code from htmllint fails the product.

This processor supports batch mode.

htmllint reads `.htmllintrc` from the working directory (unless `--rc` names
another file), so the project root's `.htmllintrc` is an input of every
product when it exists (`dep_auto`): an edit re-checks every file, with no
`dep_inputs` entry needed.

## Source Files

- Input: `**/*.html`, `**/*.htm`
- Output: none (checker)

## Configuration

```toml
[processor.checker.htmllint]
args = []
dep_inputs = []
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `args` | string[] | `[]` | Extra arguments passed to htmllint |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |
| `dep_auto` | string[] | `[".htmllintrc"]` | Config files that are inputs when they exist |

## Batch support

The tool accepts multiple files on the command line. When batching is enabled (default), rsconstruct passes all files in a single invocation for better performance.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
