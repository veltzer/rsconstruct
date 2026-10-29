# Pdflatex Processor

## Purpose

Compiles LaTeX documents to PDF using [pdflatex](https://www.tug.org/applications/pdftex/).

## How It Works

Discovers `.tex` files in the project and runs `pdflatex` on each file. Runs
multiple compilation passes (configurable) to resolve cross-references and
table of contents. Optionally uses `qpdf` to linearize the output PDF.

## Source Files

- Input: `**/*.tex`
- Output: `out/processor.generator.pdflatex/{relative_path}.pdf`

## Configuration

```toml
[processor.generator.pdflatex]
command = "pdflatex"                   # The pdflatex command to run
runs = 2                               # Number of compilation passes
qpdf = true                           # Use qpdf to linearize output PDF
shell_escape = true                    # Pass -shell-escape to pdflatex
args = []                              # Additional arguments to pass to pdflatex
output_dir = "out/processor.generator.pdflatex"            # Output directory
dep_inputs = []                      # Additional files that trigger rebuilds when changed
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `command` | string | `"pdflatex"` | The pdflatex executable to run |
| `runs` | integer | `2` | Number of compilation passes (for cross-references) |
| `qpdf` | bool | `true` | Use qpdf to linearize the output PDF |
| `shell_escape` | bool | `true` | Pass `-shell-escape` to pdflatex, which lets `.tex` files run shell commands (packages like `minted` need it); set to `false` for untrusted sources |
| `args` | string[] | `[]` | Extra arguments passed to pdflatex |
| `output_dir` | string | `"out/processor.generator.pdflatex"` | Output directory for PDF files |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |
| `required_tools` | string[] | `[]` | Extra tools the build needs on PATH beyond the built-in ones (e.g. what a wrapper script shells out to); checked before building and covered by `tool install` and version locking |

## Installation

pdflatex is part of TeX Live, a system package installed with
`rsconstruct tool install machine`. With `qpdf = true`, `rsconstruct tool
install project` downloads the qpdf release from the qpdf GitHub releases
into `.rsconstruct/tools`, at the version rsconstruct.lock pins. The
release is kept whole: qpdf loads its own libraries from the `lib/` beside
its `bin/`. `rsconstruct tool install machine qpdf` installs the
distribution's package with apt instead.

## Batch support

Each input file is processed individually, producing its own output file.

## Clean behavior

This processor is a Generator — `rsconstruct clean outputs` removes each declared output file individually with no directory recursion. After all per-product cleans complete, the orchestrator removes any parent directories that are now empty. Pass `--no-empty-dirs` to keep them. See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
