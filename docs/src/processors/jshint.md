# JSHint Processor

## Purpose

Lints JavaScript files using [JSHint](https://jshint.com/).

## How It Works

Discovers `.js`, `.jsx`, `.mjs`, and `.cjs` files in the project (excluding
common build tool directories), runs `jshint` on each file, and records success
in the cache. A non-zero exit code from jshint fails the product.

This processor supports batch mode.

If a `.jshintrc` file exists, it is automatically added as an extra input so
that configuration changes trigger rebuilds.

## Source Files

- Input: `**/*.js`, `**/*.jsx`, `**/*.mjs`, `**/*.cjs`
- Output: none (checker)

## Configuration

```toml
[processor.jshint]
linter = "jshint"
args = []
extra_inputs = []
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `linter` | string | `"jshint"` | The jshint executable to run |
| `args` | string[] | `[]` | Extra arguments passed to jshint |
| `extra_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

The tool accepts multiple files on the command line. When batching is enabled (default), rsconstruct passes all files in a single invocation for better performance.
