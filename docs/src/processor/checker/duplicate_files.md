# Duplicate Files Processor

## Purpose

Finds files with identical content. Native (in-process, SHA-256; no external tools required).

## How It Works

Unlike most checkers, this processor creates **one product for the whole set** of scanned files, because duplication is a property of the set, not of a single file. Every file is hashed with SHA-256, and the product fails if two or more files share a hash, listing each group:

```
src/__init__.py: [processor.checker.duplicate_files] 2 set(s) of duplicate files found:
  src/__init__.py, src/empty.py
  src/b.sh, src/c.sh
```

The error is reported under the first file of the product. Since any change to any scanned file changes the product's inputs, the whole set is re-checked whenever one file changes.

All empty files have the same hash, so **two empty files are duplicates** (for example two empty `__init__.py` files in different packages). Exclude them with `src_exclude_files`, or narrow `src_dirs`.

## Source Files

- Input: files under `src_dirs` with one of the default extensions: `.py .rs .js .ts .c .cc .h .hh .java .rb .go .sh .md .yaml .yml .json .toml .xml .html .css`
- Output: none (checker)

The default `src_dirs` is empty, so a stanza with no `src_dirs` or `src_files` checks nothing: name the directories (`[""]` for the whole project) or files to check.

## Configuration

```toml
[processor.checker.duplicate_files]
src_dirs = ["src"]
src_exclude_files = ["__init__.py"]
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `src_dirs` | string[] | `[]` | Directories to scan |
| `src_extensions` | string[] | (list above) | File extensions to compare |
| `src_exclude_files` | string[] | `[]` | File names to leave out of the comparison |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

Not applicable: all scanned files form a single product.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
