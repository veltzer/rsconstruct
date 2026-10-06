# Encoding Processor

## Purpose

Checks that text files are valid UTF-8 without a byte order mark. Native (in-process; no external tools required).

## How It Works

Each file is read as bytes and rejected if it:

- starts with a UTF-8 BOM (`EF BB BF`): `file has UTF-8 BOM (byte order mark)`
- starts with a UTF-16 BOM (`FF FE` or `FE FF`): `file appears to be UTF-16 encoded`
- contains bytes that are not valid UTF-8: `invalid UTF-8 at byte 5 (line 1)`

```
src/bom.py: [processor.checker.encoding] Encoding errors found:
src/bom.py: file has UTF-8 BOM (byte order mark)
```

## Source Files

- Input: files under `src_dirs` with one of the default text extensions: `.py .rs .js .ts .c .cc .h .hh .java .rb .go .sh .bash .lua .pl .pm .php .md .yaml .yml .json .toml .xml .html .htm .css .scss .sass .tex .txt`
- Output: none (checker)

The default `src_dirs` is empty, so a stanza with no `src_dirs` or `src_files` checks nothing: name the directories (`[""]` for the whole project) or files to check.

## Configuration

```toml
[processor.checker.encoding]
src_dirs = ["src", "docs"]
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `src_dirs` | string[] | `[]` | Directories to scan |
| `src_extensions` | string[] | (list above) | File extensions to check |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

Files are checked individually within a batch, so one bad file fails only its own product.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
