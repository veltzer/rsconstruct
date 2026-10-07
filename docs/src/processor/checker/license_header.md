# License Header Processor

## Purpose

Checks that source files carry a license header. Native (in-process; no external tools required).

## How It Works

Each file must **start with** the header: the lines of `header_lines`, every one of them, in order, each matching a whole line exactly and ending with a newline. Line endings are compared after reading `\r\n` and `\r` as `\n`. Two things may come before the header:

- **A shebang line** (`#!...`), skipped when `skip_shebang` is on (the default), so scripts can keep it first.
- **`optional_prefix_lines`**, all of them or none, matched exactly. This is how kernel sources carry the SPDX line the kernel requires first, followed by the project's license block. One stanza then covers ordinary sources (header at the top) and kernel sources (SPDX line, then header).

The SPDX line can also be the header itself: with `header_lines = ["// SPDX-License-Identifier: GPL-2.0"]`, every file must open with exactly that line.

The first line that differs is reported, with its line number in the file and its number in the header. When a file starts with the prefix, the mismatch is reported in the header after it:

```text
src/user.c:1: license header line 1 differs: expected "// SPDX-License-Identifier: GPL-2.0", found "/*"
src/broken.c:3: license header line 2 differs: expected " * This file is part of the demo package.", found " * Some other package."
src/short.c:17: file ends without a newline after license header line 17
```

A file that is not valid UTF-8 fails with a read error. `header_lines` is required and must not be empty.

## Source Files

- Input: files under `src_dirs` with one of the default extensions: `.py .rs .js .ts .c .cc .h .hh .java .rb .go .sh .bash`
- Output: none (checker)

The default `src_dirs` is empty, so a stanza with no `src_dirs` or `src_files` checks nothing: name the directories (`[""]` for the whole project) or files to check.

## Configuration

```toml
# Kernel sources must open with the SPDX line
[processor.checker.license_header.kernel]
src_dirs = ["src/kernel_standalone"]
src_extensions = [".c", ".h"]
header_lines = ["// SPDX-License-Identifier: GPL-2.0"]

# Every source must start with the project's license block, kernel
# sources with their SPDX line before it. Shortened here: list every line
# of the real header, since all of them must match.
[processor.checker.license_header.block]
src_dirs = ["src"]
src_extensions = [".c", ".cc", ".h", ".hh"]
skip_shebang = false
optional_prefix_lines = ["// SPDX-License-Identifier: GPL-2.0"]
header_lines = [
    "/*",
    " * This file is part of the demos-os-linux package.",
    " * Copyright (C) 2011-2026 Mark Veltzer <mark.veltzer@gmail.com>",
    " */",
]
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `header_lines` | string[] | (required) | The lines every file must start with, one entry per line |
| `optional_prefix_lines` | string[] | `[]` | Lines that may come right before the header, all or none |
| `skip_shebang` | bool | `true` | Skip a leading `#!` line before looking for the header |
| `src_dirs` | string[] | `[]` | Directories to scan |
| `src_extensions` | string[] | (list above) | File extensions to check |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

Changing `header_lines`, `optional_prefix_lines` or `skip_shebang` re-checks every file.

## Batch support

Files are checked individually within a batch, so one bad file fails only its own product.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
