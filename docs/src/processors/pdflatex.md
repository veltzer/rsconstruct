# Pdflatex Processor

## Purpose

Compiles LaTeX documents to PDF using [pdflatex](https://www.tug.org/applications/pdftex/).

## How It Works

Discovers `.tex` files in the project and runs `pdflatex` on each file. Runs
multiple compilation passes (configurable) to resolve cross-references and
table of contents. Optionally uses `qpdf` to linearize the output PDF.

## Source Files

- Input: `**/*.tex`
- Output: `out/pdflatex/{relative_path}.pdf`

## Configuration

```toml
[processor.pdflatex]
pdflatex = "pdflatex"                  # The pdflatex command to run
runs = 2                               # Number of compilation passes
qpdf = true                           # Use qpdf to linearize output PDF
args = []                              # Additional arguments to pass to pdflatex
output_dir = "out/pdflatex"            # Output directory
extra_inputs = []                      # Additional files that trigger rebuilds when changed
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `pdflatex` | string | `"pdflatex"` | The pdflatex executable to run |
| `runs` | integer | `2` | Number of compilation passes (for cross-references) |
| `qpdf` | bool | `true` | Use qpdf to linearize the output PDF |
| `args` | string[] | `[]` | Extra arguments passed to pdflatex |
| `output_dir` | string | `"out/pdflatex"` | Output directory for PDF files |
| `extra_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |
