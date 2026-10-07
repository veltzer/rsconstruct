# Ijsonlint Processor

## Purpose

Checks that JSON files parse. Native (in-process, using `serde_json`; no external tools required). A Rust alternative to [jsonlint](jsonlint.md).

## How It Works

Each file is read and parsed as JSON. A file that does not parse fails its product with the parser's message and position:

```text
data/bad.json: [processor.checker.ijsonlint] Invalid JSON:
data/bad.json: trailing comma at line 1 column 9
```

This is a well-formedness check only. There are no style rules, and duplicate keys are accepted (the last value wins).

## Source Files

- Input: `.json` files under `src_dirs`
- Output: none (checker)

The default `src_dirs` is empty, so a stanza with no `src_dirs` or `src_files` checks nothing: name the directories (`[""]` for the whole project) or files to check.

## Configuration

```toml
[processor.checker.ijsonlint]
src_dirs = [""]
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `src_dirs` | string[] | `[]` | Directories to scan |
| `src_extensions` | string[] | `[".json"]` | File extensions to check |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

Files are checked individually within a batch, so one bad file fails only its own product.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
