# Marp Processor

## Purpose

Converts Markdown slides to PDF, PPTX, or HTML using [Marp](https://marp.app/).

## How It Works

Discovers `.md` files in the project and runs `marp` on each file, generating
output in the configured formats. Each format produces a separate output file.

## Source Files

- Input: `**/*.md`
- Output: `out/marp/{format}/{relative_path}.{format}`

## Configuration

```toml
[processor.marp]
marp_bin = "marp"                      # The marp command to run
formats = ["pdf"]                      # Output formats (pdf, pptx, html)
args = ["--html", "--allow-local-files"]  # Additional arguments to pass to marp
output_dir = "out/marp"                # Output directory
extra_inputs = []                      # Additional files that trigger rebuilds when changed
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `marp_bin` | string | `"marp"` | The marp executable to run |
| `formats` | string[] | `["pdf"]` | Output formats to generate (`pdf`, `pptx`, `html`) |
| `args` | string[] | `["--html", "--allow-local-files"]` | Extra arguments passed to marp |
| `output_dir` | string | `"out/marp"` | Base output directory |
| `extra_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

Each input file is processed individually, producing its own output file.
