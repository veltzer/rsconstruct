# Ijq Processor

## Purpose

Checks that JSON files parse. Native (in-process, using `serde_json`; no external tools required). It is the in-process counterpart of [jq](jq.md), but it only parses: no jq filter is evaluated.

## How It Works

Each file is read and parsed as JSON. A file that does not parse fails its product with an `Invalid JSON:` message giving the position.

The check is identical to [ijsonlint](ijsonlint.md); use whichever name reads better in your config. Duplicate keys are accepted (the last value wins).

## Source Files

- Input: `.json` files under `src_dirs`
- Output: none (checker)

The default `src_dirs` is empty, so a stanza with no `src_dirs` or `src_files` checks nothing: name the directories (`[""]` for the whole project) or files to check.

## Configuration

```toml
[processor.checker.ijq]
src_dirs = ["data"]
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
