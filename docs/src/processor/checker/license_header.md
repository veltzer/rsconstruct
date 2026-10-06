# License Header Processor

## Purpose

Checks that source files carry a license header. Native (in-process; no external tools required).

## How It Works

Each file must **start with** the lines of `header_lines`, every one of them, in order, each matching a whole line exactly. A shebang line (`#!...`) before the header is allowed.

Kernel sources open with an SPDX line, and the check supports the two ways it is used:

- **The SPDX line is the header.** With `header_lines = ["// SPDX-License-Identifier: GPL-2.0"]`, every file must open with that line, which is what the Linux kernel requires of its sources.
- **A license block follows the SPDX line.** When the header is something else (a comment block, say), a file may have an SPDX line first and the header right after it. One configuration then covers both ordinary sources (`header` at the top) and kernel sources (`SPDX line`, then `header`).

The first line that differs is reported, with its line number in the file and its number in the header:

```
src/user.c:1: license header line 1 differs: expected "// SPDX-License-Identifier: GPL-2.0", found "/*"
src/broken.c:3: license header line 2 differs: expected " * This file is part of the demo package.", found " * Some other package."
```

`header_lines` is required and must not be empty.

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

# Every source must start with the project's license block
# (kernel sources may put their SPDX line before it). Shortened here:
# list every line of the real header, since all of them must match.
[processor.checker.license_header.block]
src_dirs = ["src"]
src_extensions = [".c", ".cc", ".h", ".hh"]
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
| `src_dirs` | string[] | `[]` | Directories to scan |
| `src_extensions` | string[] | (list above) | File extensions to check |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

Changing `header_lines` re-checks every file.

## Batch support

Files are checked individually within a batch, so one bad file fails only its own product.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
