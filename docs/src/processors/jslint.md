# JSLint Processor

## Purpose

Lints JavaScript files using [JSLint](https://www.jslint.com/).

## How It Works

Discovers `.js` files in the project (excluding common build tool directories),
runs `jslint` on each file, and records success in the cache. A non-zero exit
code from jslint fails the product.

This processor supports batch mode.

## Source Files

- Input: `**/*.js`
- Output: none (checker)

## Configuration

```toml
[processor.jslint]
args = []
dep_inputs = []
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `args` | string[] | `[]` | Extra arguments passed to jslint |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

The tool accepts multiple files on the command line. When batching is enabled (default), rsconstruct passes all files in a single invocation for better performance.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../processors.md#clean-behavior) and [`rsconstruct clean`](../commands.md#rsconstruct-clean).
