# Drawio Processor

## Purpose

Converts [Draw.io](https://www.drawio.com/) diagram files to PNG, SVG, or PDF.

## How It Works

Discovers `.drawio` files in the project and runs `drawio` in export mode on
each file, generating output in the configured formats.

## Source Files

- Input: `**/*.drawio`
- Output: `out/drawio/{format}/{relative_path}.{format}`

## Configuration

```toml
[processor.drawio]
drawio_bin = "drawio"                  # The drawio command to run
formats = ["png"]                      # Output formats (png, svg, pdf)
args = []                              # Additional arguments to pass to drawio
output_dir = "out/drawio"              # Output directory
extra_inputs = []                      # Additional files that trigger rebuilds when changed
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `drawio_bin` | string | `"drawio"` | The drawio executable to run |
| `formats` | string[] | `["png"]` | Output formats to generate (`png`, `svg`, `pdf`) |
| `args` | string[] | `[]` | Extra arguments passed to drawio |
| `output_dir` | string | `"out/drawio"` | Base output directory |
| `extra_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

Each input file is processed individually, producing its own output file.
