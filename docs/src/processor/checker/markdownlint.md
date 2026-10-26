# Markdownlint Processor

## Purpose

Lints Markdown files using [markdownlint](https://github.com/DavidAnson/markdownlint) (Node.js).

The Rust alternative is [rumdl](rumdl.md), a Markdown linter written in Rust with markdownlint's rules.

## How It Works

Discovers `.md` files in the project and runs `markdownlint` on each file. A
non-zero exit code fails the product.

Depends on the npm processor — uses the `markdownlint` binary installed by npm.

markdownlint-cli looks for its configuration in the working directory —
`.markdownlint.jsonc`, `.markdownlint.json`, `.markdownlint.yaml`,
`.markdownlint.yml` — and reads `.markdownlintignore` by default, so each of
those is an input of every product when it exists (`dep_auto`): an edit
re-checks every file, with no `dep_inputs` entry needed. A file named with
`--config` in `args` is not tracked; list it in `dep_inputs`.

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
| `required_tools` | string[] | `[]` | Extra tools the build needs on PATH beyond the built-in ones (e.g. what a wrapper script shells out to); checked before building and covered by `tool install` and version locking |
| `dep_auto` | string[] | `[".markdownlint.jsonc", ".markdownlint.json", ".markdownlint.yaml", ".markdownlint.yml", ".markdownlintignore"]` | Config files that are inputs when they exist |

## Batch support

The tool processes one file at a time. Each file is checked in a separate invocation.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
