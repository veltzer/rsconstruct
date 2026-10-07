# Itaplo Processor

## Purpose

Checks that TOML files parse. Native (in-process, using the `toml` crate; no external tools required). An in-process alternative to [taplo](taplo.md).

## How It Works

Each file is read and parsed as TOML. A file that does not parse fails its product with the parser's message and a pointer to the position:

```text
bad.toml: [processor.checker.itaplo] Invalid TOML:
bad.toml: TOML parse error at line 1, column 5
  |
1 | a =
  |     ^
string values must be quoted, expected literal string
```

Only syntax is checked. Unlike `taplo check`, there is no schema validation and no `.taplo.toml` is read.

## Source Files

- Input: `.toml` files under `src_dirs`
- Output: none (checker)

The default `src_dirs` is empty, so a stanza with no `src_dirs` or `src_files` checks nothing: name the directories (`[""]` for the whole project) or files to check.

## Configuration

```toml
[processor.checker.itaplo]
src_dirs = [""]
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `src_dirs` | string[] | `[]` | Directories to scan |
| `src_extensions` | string[] | `[".toml"]` | File extensions to check |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

Files are checked individually within a batch, so one bad file fails only its own product.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
