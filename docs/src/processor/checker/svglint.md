# Svglint Processor

## Purpose

Lints SVG files using [svglint](https://github.com/birjj/svglint).

## How It Works

Runs `svglint <files>`. A non-zero exit fails the product. With no `.svglintrc.js`, svglint only checks that each file is valid SVG/XML:

```
x img/bad.svg
  x valid Expected closing tag 'g' (opened in line 1, col 6) instead of closing
           tag 'svg'.
```

Rules are configured in `.svglintrc.js`, which is an input when it exists (`dep_auto`), so editing it re-checks every file.

## Source Files

- Input: `.svg` files under `src_dirs`
- Output: none (checker)

The default `src_dirs` is empty, so a stanza with no `src_dirs` or `src_files` checks nothing: name the directories (`[""]` for the whole project) or files to check.

## Configuration

```toml
[processor.checker.svglint]
src_dirs = ["images"]
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `command` | string | `"svglint"` | The svglint executable to run |
| `args` | string[] | `[]` | Extra arguments passed to svglint |
| `src_dirs` | string[] | `[]` | Directories to scan |
| `src_extensions` | string[] | `[".svg"]` | File extensions to check |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |
| `dep_auto` | string[] | `[".svglintrc.js"]` | Config files that are inputs when they exist |

## Batch support

The tool accepts multiple files on the command line. When batching is enabled (default), rsconstruct passes all files in a single invocation; a failure then marks every file in that invocation as failed (see `batch` in [Configuration](../../configuration.md)). Set `batch = false` to have each file reported on its own.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
