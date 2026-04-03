# ESLint Processor

## Purpose

Lints JavaScript and TypeScript files using [ESLint](https://eslint.org/).

## How It Works

Discovers `.js`, `.jsx`, `.ts`, `.tsx`, `.mjs`, and `.cjs` files in the project
(excluding common build tool directories), runs `eslint` on each file, and records
success in the cache. A non-zero exit code from eslint fails the product.

This processor supports batch mode, allowing multiple files to be checked in a
single eslint invocation for better performance.

If an ESLint config file exists (`.eslintrc*` or `eslint.config.*`), it is
automatically added as an extra input so that configuration changes trigger rebuilds.

## Source Files

- Input: `**/*.js`, `**/*.jsx`, `**/*.ts`, `**/*.tsx`, `**/*.mjs`, `**/*.cjs`
- Output: none (checker)

## Configuration

```toml
[processor.eslint]
linter = "eslint"
args = []
extra_inputs = []
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `linter` | string | `"eslint"` | The eslint executable to run |
| `args` | string[] | `[]` | Extra arguments passed to eslint |
| `extra_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

The tool accepts multiple files on the command line. When batching is enabled (default), rsconstruct passes all files in a single invocation for better performance.
