# Tectonic Processor

## Purpose

Compiles LaTeX documents to PDF using
[tectonic](https://tectonic-typesetting.github.io/), a Rust tool: the
counterpart of the [pdflatex](pdflatex.md) processor that needs no TeX
distribution on the machine.

## How It Works

Discovers `.tex` files in the project and runs `tectonic` on each. tectonic
reruns the engine by itself until cross-references and the table of contents
settle, so there is no pass count to configure, and it keeps its `.aux` and
`.log` files in memory, so nothing but the PDF is written.

What differs from pdflatex:

- **The engine is XeTeX**, not pdfTeX. Most documents compile unchanged; one
  that uses pdfTeX primitives (`\pdfoutput`, `\pdftrailerid`) or relies on
  `inputenc` byte handling may not.
- **TeX files are downloaded on first use.** tectonic ships no packages: the
  classes, packages and fonts a document needs are fetched from its bundle
  the first time they are used and cached under `~/.cache/tectonic`, so the
  first build needs the network and takes longer. The processor's tool is
  declared as needing the network, which the sandbox therefore leaves on.
  Pass `args = ["--only-cached"]` to forbid downloads once the cache is warm.
- **No `-shell-escape` by default.** Add `args = ["-Z", "shell-escape"]` for
  packages such as `minted` that need it.

The PDF is reproducible: the processor runs tectonic with
`SOURCE_DATE_EPOCH=0`, so the creation date and the document ID do not come
from the clock, and `\today` prints 1 January 1970. A document that prints
the date must set it with `\date{...}`.

## Source Files

- Input: `**/*.tex`
- Output: `out/processor.generator.tectonic/{relative_path}.pdf`

## Configuration

```toml
[processor.generator.tectonic]
command = "tectonic"                   # The tectonic command to run
args = []                              # Additional arguments to pass to tectonic
output_dir = "out/processor.generator.tectonic"            # Output directory
dep_inputs = []                      # Additional files that trigger rebuilds when changed
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `command` | string | `"tectonic"` | The tectonic executable to run |
| `args` | string[] | `[]` | Extra arguments passed to tectonic |
| `output_dir` | string | `"out/processor.generator.tectonic"` | Output directory for PDF files |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds (files a document `\input`s or `\includegraphics`) |
| `required_tools` | string[] | `[]` | Extra tools the build needs on PATH beyond the built-in ones (e.g. what a wrapper script shells out to); checked before building and covered by `tool install` and version locking |

## Installation

`rsconstruct tool install project` downloads the tectonic release from its
GitHub releases into `.rsconstruct/tools`, at the version rsconstruct.lock
pins. It is the statically linked build, so it needs no system libraries.

## Batch support

Each input file is processed individually, producing its own output file.

## Clean behavior

This processor is a Generator — `rsconstruct clean outputs` removes each declared output file individually with no directory recursion. After all per-product cleans complete, the orchestrator removes any parent directories that are now empty. Pass `--no-empty-dirs` to keep them. See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
