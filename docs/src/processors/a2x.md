# A2x Processor

## Purpose

Converts AsciiDoc files to PDF (or other formats) using [a2x](https://asciidoc-py.github.io/).

## How It Works

Discovers `.txt` (AsciiDoc) files in the project and runs `a2x` on each file,
producing output in the configured format.

## Source Files

- Input: `**/*.txt`
- Output: `out/a2x/{relative_path}.pdf`

## Configuration

```toml
[processor.a2x]
a2x = "a2x"                           # The a2x command to run
format = "pdf"                         # Output format (pdf, xhtml, dvi, ps, epub, mobi)
args = []                              # Additional arguments to pass to a2x
output_dir = "out/a2x"                # Output directory
extra_inputs = []                      # Additional files that trigger rebuilds when changed
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `a2x` | string | `"a2x"` | The a2x executable to run |
| `format` | string | `"pdf"` | Output format |
| `args` | string[] | `[]` | Extra arguments passed to a2x |
| `output_dir` | string | `"out/a2x"` | Output directory |
| `extra_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

Each input file is processed individually, producing its own output file.
