# Svgo Processor

## Purpose

Checks that SVG files are well-formed, using [svgo](https://svgo.dev/)'s parser.

## How It Works

Runs `svgo --quiet -o - -i <file>` for each file. svgo optimizes the file and writes the result to stdout, which is discarded; the source file is never modified. Only the exit code matters: svgo fails when it cannot parse the file, and that fails the product:

```text
SvgoParserError: img/bad.svg:1:14: Unexpected close tag

> 1 | <svg><g></svg>
    |              ^
```

This is a well-formedness check, not a check that the file is already optimized. An `svgo.config.js`, `svgo.config.mjs` or `svgo.config.cjs` that exists is an input (`dep_auto`).

## Source Files

- Input: `.svg` files under `src_dirs`
- Output: none (checker)

The default `src_dirs` is empty, so a stanza with no `src_dirs` or `src_files` checks nothing: name the directories (`[""]` for the whole project) or files to check.

## Configuration

```toml
[processor.checker.svgo]
src_dirs = ["images"]
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `command` | string | `"svgo"` | The svgo executable to run |
| `args` | string[] | `[]` | Extra arguments passed to svgo |
| `src_dirs` | string[] | `[]` | Directories to scan |
| `src_extensions` | string[] | `[".svg"]` | File extensions to check |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |
| `dep_auto` | string[] | `["svgo.config.js", "svgo.config.mjs", "svgo.config.cjs"]` | Config files that are inputs when they exist |

## Batch support

Not supported: svgo needs one output per input, and the processor gives it the single output `-` (stdout), so each file is checked in its own invocation.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
