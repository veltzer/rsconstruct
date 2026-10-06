# License Header Processor

## Purpose

Checks that source files carry a license header. Native (in-process; no external tools required).

## How It Works

For each file, the processor looks for a line containing **any one** of the strings in `header_lines`. A file passes if it finds one, and fails if none of them occurs:

```
src/a.sh: [processor.checker.license_header] 1 file(s) missing license headers:
src/a.sh: missing license header (expected one of: SPDX-License-Identifier)
```

Be aware of what the check does **not** do:

- It searches the **whole file**, not just its top: a matching line anywhere passes.
- It needs **one** of the strings, not all of them: `header_lines` is a list of alternatives, not a multi-line header to match in full.
- With `header_lines` empty (the default), **every file passes**. Always set it.

Matching is plain substring matching, so a short, distinctive marker works best (`SPDX-License-Identifier`, `Copyright (c) Example Corp`).

## Source Files

- Input: files under `src_dirs` with one of the default extensions: `.py .rs .js .ts .c .cc .h .hh .java .rb .go .sh .bash`
- Output: none (checker)

The default `src_dirs` is empty, so a stanza with no `src_dirs` or `src_files` checks nothing: name the directories (`[""]` for the whole project) or files to check.

## Configuration

```toml
[processor.checker.license_header]
src_dirs = ["src"]
header_lines = ["SPDX-License-Identifier"]
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `header_lines` | string[] | `[]` | Strings, one of which must appear in each file. Empty means no check |
| `src_dirs` | string[] | `[]` | Directories to scan |
| `src_extensions` | string[] | (list above) | File extensions to check |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

Changing `header_lines` re-checks every file.

## Batch support

Files are checked individually within a batch, so one bad file fails only its own product.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
