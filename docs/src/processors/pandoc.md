# Pandoc Processor

## Purpose

Converts documents between formats using [pandoc](https://pandoc.org/).

## How It Works

Discovers `.md` files in the project and runs `pandoc` on each file, converting
from the configured source format to the configured output formats.

## Source Files

- Input: `**/*.md`
- Output: `out/pandoc/{format}/{relative_path}.{format}`

## Configuration

```toml
[processor.pandoc]
pandoc = "pandoc"                      # The pandoc command to run
from = "markdown"                      # Source format
formats = ["pdf"]                      # Output formats (pdf, docx, html, etc.)
args = []                              # Additional arguments to pass to pandoc
output_dir = "out/pandoc"              # Output directory
extra_inputs = []                      # Additional files that trigger rebuilds when changed
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `pandoc` | string | `"pandoc"` | The pandoc executable to run |
| `from` | string | `"markdown"` | Source format |
| `formats` | string[] | `["pdf"]` | Output formats to generate |
| `args` | string[] | `[]` | Extra arguments passed to pandoc |
| `output_dir` | string | `"out/pandoc"` | Base output directory |
| `extra_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

Each input file is processed individually, producing its own output file.
