# Biome Processor

## Purpose

Lints CSS, JavaScript, TypeScript and JSON files using [biome](https://biomejs.dev/).
It is the Rust alternative to the [Stylelint](stylelint.md) processor, and a
second alternative, next to [oxlint](oxlint.md), to [ESLint](eslint.md): a
single static binary that needs no node runtime and no `node_modules/`.

biome does not parse SCSS, Sass or Less. A repo with those stays on
stylelint for them.

## How It Works

Discovers `.css`, `.js`, `.jsx`, `.ts`, `.tsx`, `.mjs`, `.cjs`, `.json` and
`.jsonc` files in the project (excluding common build tool directories), runs
`biome lint` on each file, and records success in the cache. A non-zero exit
code from biome fails the product.

This processor supports batch mode, allowing multiple files to be checked in a
single biome invocation for better performance.

If a biome config file exists (`biome.json` or `biome.jsonc`), it is
automatically added as an extra input so that configuration changes trigger
rebuilds.

## Choosing what biome lints

biome has one linter switch per language in its config
(`javascript.linter.enabled`, `json.linter.enabled`, ...). A repo that lints
JavaScript with oxlint turns biome's JavaScript linter off in `biome.jsonc`
and narrows this processor to stylesheets:

```toml
[processor.checker.biome]
src_dirs = ["src"]
src_extensions = [".css"]
```

Keep the two in step: biome exits non-zero when every file it was handed
belongs to a language whose linter is off ("No files were processed").

## Migrating from stylelint

Rule names differ but the coverage is close. Of stylelint's standard
correctness rules, three need no biome rule because biome's CSS parser
rejects the construct outright (`color-no-invalid-hex`,
`no-invalid-double-slash-comments`, `string-no-newline`), one has no
equivalent (`function-calc-no-unspaced-operator`), and `no-empty-source`
differs in that biome's `noEmptySource` flags a comment-only stylesheet
where stylelint does not. `no-duplicate-selectors` maps to
`noDuplicateSelectors`, still a nursery rule that has to be enabled
explicitly. `biome explain <rule>` shows any rule's group and options.

## Source Files

- Input: `**/*.css`, `**/*.js`, `**/*.jsx`, `**/*.ts`, `**/*.tsx`, `**/*.mjs`, `**/*.cjs`, `**/*.json`, `**/*.jsonc`
- Output: none (checker)

## Configuration

```toml
[processor.checker.biome]
command = "biome"
args = []
dep_inputs = []
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `command` | string | `"biome"` | The biome executable to run |
| `args` | string[] | `[]` | Extra arguments passed after `lint` and before the file paths |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |
| `dep_auto` | string[] | `biome.json`, `biome.jsonc` | Config files auto-detected as inputs |

## Installation

`rsconstruct tool install` downloads the static Linux binary from the latest
biome GitHub release. The npm package is the fallback.

## Batch support

The tool accepts multiple files on the command line. When batching is enabled (default), rsconstruct passes all files in a single invocation for better performance.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
