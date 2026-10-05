# Oxlint Processor

## Purpose

Lints JavaScript and TypeScript files using [oxlint](https://oxc.rs/docs/guide/usage/linter.html),
the linter from the oxc project. It is the Rust alternative to the
[ESLint](eslint.md) processor: a single static binary that needs no node
runtime and no `node_modules/`.

## How It Works

Discovers `.js`, `.jsx`, `.ts`, `.tsx`, `.mjs`, and `.cjs` files in the project
(excluding common build tool directories), runs `oxlint` on each file, and
records success in the cache. A non-zero exit code from oxlint fails the
product.

This processor supports batch mode, allowing multiple files to be checked in a
single oxlint invocation for better performance.

If an oxlint config file exists (`.oxlintrc.json` or `oxlint.config.*`), it is
automatically added as an extra input so that configuration changes trigger
rebuilds.

## Migrating from ESLint

oxlint reads an ESLint-style config and honours `eslint-disable` comments, so
a repo moves over by translating its rule list once into `.oxlintrc.json`.
Rule names are the same; two rules from `eslint:recommended` do not exist in
oxlint because its parser rejects the constructs as syntax errors:
`no-dupe-args` and `no-octal`. oxlint refuses to start on an unknown rule
name, so these must be left out of the translated config.

Note that oxlint parses `.js` files with JSX enabled, so a file that ESLint
rejected for stray HTML may parse cleanly under oxlint.

## Source Files

- Input: `**/*.js`, `**/*.jsx`, `**/*.ts`, `**/*.tsx`, `**/*.mjs`, `**/*.cjs`
- Output: none (checker)

## Configuration

```toml
[processor.checker.oxlint]
command = "oxlint"
args = ["-c", ".oxlintrc.json"]
dep_inputs = []
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `command` | string | `"oxlint"` | The oxlint executable to run |
| `args` | string[] | `[]` | Extra arguments passed to oxlint |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |
| `dep_auto` | string[] | `.oxlintrc.json`, `oxlint.config.*` | Config files auto-detected as inputs |

## Installation

`rsconstruct tool install` downloads the static Linux binary from the oxc
GitHub release. The npm package and the crate are the fallbacks.

## Batch support

The tool accepts multiple files on the command line. When batching is enabled (default), rsconstruct passes all files in a single invocation for better performance.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
