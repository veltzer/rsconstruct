# Stylelint Processor

## Purpose

Lints CSS, SCSS, Sass, and Less files using [stylelint](https://stylelint.io/).

## How It Works

Discovers `.css`, `.scss`, `.sass`, and `.less` files in the project (excluding
common build tool directories), runs `stylelint` on each file, and records success
in the cache. A non-zero exit code from stylelint fails the product.

This processor supports batch mode.

If a stylelint config file exists (`.stylelintrc*` or `stylelint.config.*`), it
is automatically added as an extra input so that configuration changes trigger
rebuilds.

## Source Files

- Input: `**/*.css`, `**/*.scss`, `**/*.sass`, `**/*.less`
- Output: none (checker)

## Configuration

```toml
[processor.stylelint]
linter = "stylelint"
args = []
extra_inputs = []
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `linter` | string | `"stylelint"` | The stylelint executable to run |
| `args` | string[] | `[]` | Extra arguments passed to stylelint |
| `extra_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

The tool accepts multiple files on the command line. When batching is enabled (default), rsconstruct passes all files in a single invocation for better performance.
