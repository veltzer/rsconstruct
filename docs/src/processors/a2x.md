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
dep_inputs = []                      # Additional files that trigger rebuilds when changed
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `a2x` | string | `"a2x"` | The a2x executable to run |
| `format` | string | `"pdf"` | Output format |
| `args` | string[] | `[]` | Extra arguments passed to a2x |
| `output_dir` | string | `"out/a2x"` | Output directory |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

Each input file is processed individually, producing its own output file.

## Clean behavior

This processor is a Generator — `rsconstruct clean outputs` removes each declared output file individually with no directory recursion. After all per-product cleans complete, the orchestrator removes any parent directories that are now empty. Pass `--no-empty-dirs` to keep them. See [Clean behavior](../processors.md#clean-behavior) and [`rsconstruct clean`](../commands.md#rsconstruct-clean).
