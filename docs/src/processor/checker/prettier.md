# Prettier Processor

## Purpose

Checks that files are formatted the way [Prettier](https://prettier.io/) would format them.

## How It Works

Runs `prettier --check <files>`. Prettier exits non-zero when a file is not formatted, which fails the product; its output names the files:

```
[warn] web/a.js
[warn] Code style issues found in the above file. Run Prettier with --write to fix.
```

Prettier's own config file is read as usual, and any of `.prettierrc`, `.prettierrc.json`, `.prettierrc.js`, `.prettierrc.yml`, `.prettierrc.yaml`, `.prettierrc.toml`, `.prettierrc.cjs`, `.prettierrc.mjs`, `prettier.config.js`, `prettier.config.cjs` and `prettier.config.mjs` that exists is an input (`dep_auto`), so editing it re-checks every file.

This processor checks only; it is not fix-capable through `rsconstruct fix`. To reformat, run `prettier --write` yourself, or wrap it in a [`script`](script.md) instance with a `fix_command`.

## Source Files

- Input: files under `src_dirs` with extensions `.js .jsx .ts .tsx .mjs .cjs .css .scss .less .html .json .md .yaml .yml`
- Output: none (checker)

The default `src_dirs` is empty, so a stanza with no `src_dirs` or `src_files` checks nothing: name the directories (`[""]` for the whole project) or files to check.

## Configuration

```toml
[processor.checker.prettier]
src_dirs = ["web"]
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `command` | string | `"prettier"` | The prettier executable to run |
| `args` | string[] | `[]` | Extra arguments passed to prettier |
| `src_dirs` | string[] | `[]` | Directories to scan |
| `src_extensions` | string[] | (list above) | File extensions to check |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |
| `dep_auto` | string[] | (config files above) | Config files that are inputs when they exist |

## Batch support

The tool accepts multiple files on the command line. When batching is enabled (default), rsconstruct passes all files in a single invocation; a failure then marks every file in that invocation as failed (see `batch` in [Configuration](../../configuration.md)). Set `batch = false` to have each file reported on its own.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
