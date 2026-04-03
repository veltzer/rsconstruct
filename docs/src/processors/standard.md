# Standard Processor

## Purpose

Checks JavaScript code style using [standard](https://standardjs.com/).

## How It Works

Discovers `.js` files in the project (excluding common build tool directories),
runs `standard` on each file, and records success in the cache. A non-zero exit
code from standard fails the product.

This processor supports batch mode.

## Source Files

- Input: `**/*.js`
- Output: none (checker)

## Configuration

```toml
[processor.standard]
args = []
extra_inputs = []
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `args` | string[] | `[]` | Extra arguments passed to standard |
| `extra_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

The tool accepts multiple files on the command line. When batching is enabled (default), rsconstruct passes all files in a single invocation for better performance.
