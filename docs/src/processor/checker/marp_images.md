# Marp Images Processor

## Purpose

Checks that every local image referenced from a Markdown file exists. Built for [Marp](../generator/marp.md) slide decks, but works on any Markdown. Native (in-process; no external tools required).

## How It Works

Every `![alt](path)` and `![alt](path "title")` reference is read from each file, line by line. References to `http://`, `https://` and `data:` URIs are skipped. Every other path is resolved **relative to the directory of the Markdown file**, and must exist:

```
slides/deck.md: [processor.checker.marp_images] Missing image references:
slides/deck.md:2: missing image: img/missing.png
```

Only inline Markdown image syntax is recognized: HTML `<img>` tags, reference-style images (`![alt][ref]`) and Marp background directives in comments are not checked.

## Source Files

- Input: `.md` files under `src_dirs`
- Output: none (checker)

The default `src_dirs` is empty, so a stanza with no `src_dirs` or `src_files` checks nothing: name the directories (`[""]` for the whole project) or files to check.

## Configuration

```toml
[processor.checker.marp_images]
src_dirs = ["marp"]
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `src_dirs` | string[] | `[]` | Directories to scan |
| `src_extensions` | string[] | `[".md"]` | File extensions to check |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

Only the Markdown files are inputs: adding or deleting an image file does not by itself re-run the check. Run `rsconstruct build` after touching the deck, or list image directories in `dep_inputs`.

## Batch support

Files are checked individually within a batch, so one bad file fails only its own product.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
