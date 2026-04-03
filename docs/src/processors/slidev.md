# Slidev Processor

## Purpose

Builds [Slidev](https://sli.dev/) presentations.

## How It Works

Discovers `.md` files in the project (excluding common build tool directories),
runs `slidev build` on each file, and records success in the cache. A non-zero
exit code from slidev fails the product.

This processor supports batch mode.

## Source Files

- Input: `**/*.md`
- Output: none (checker)

## Configuration

```toml
[processor.slidev]
args = []
extra_inputs = []
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `args` | string[] | `[]` | Extra arguments passed to slidev build |
| `extra_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

The tool accepts multiple files on the command line. When batching is enabled (default), rsconstruct passes all files in a single invocation for better performance.
