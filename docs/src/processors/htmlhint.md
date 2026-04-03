# HTMLHint Processor

## Purpose

Lints HTML files using [HTMLHint](https://htmlhint.com/).

## How It Works

Discovers `.html` and `.htm` files in the project (excluding common build tool
directories), runs `htmlhint` on each file, and records success in the cache.
A non-zero exit code from htmlhint fails the product.

This processor supports batch mode.

If a `.htmlhintrc` file exists, it is automatically added as an extra input so
that configuration changes trigger rebuilds.

## Source Files

- Input: `**/*.html`, `**/*.htm`
- Output: none (checker)

## Configuration

```toml
[processor.htmlhint]
linter = "htmlhint"
args = []
extra_inputs = []
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `linter` | string | `"htmlhint"` | The htmlhint executable to run |
| `args` | string[] | `[]` | Extra arguments passed to htmlhint |
| `extra_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

The tool accepts multiple files on the command line. When batching is enabled (default), rsconstruct passes all files in a single invocation for better performance.
