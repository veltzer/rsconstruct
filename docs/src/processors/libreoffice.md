# Libreoffice Processor

## Purpose

Converts LibreOffice documents (e.g., `.odp` presentations) to PDF or other formats.

## How It Works

Discovers `.odp` files in the project and runs `libreoffice` in headless mode
to convert each file to the configured output formats. Uses `flock` to serialize
invocations since LibreOffice only supports a single running instance.

## Source Files

- Input: `**/*.odp`
- Output: `out/libreoffice/{format}/{relative_path}.{format}`

## Configuration

```toml
[processor.libreoffice]
libreoffice_bin = "libreoffice"        # The libreoffice command to run
formats = ["pdf"]                      # Output formats (pdf, pptx)
args = []                              # Additional arguments to pass to libreoffice
output_dir = "out/libreoffice"         # Output directory
extra_inputs = []                      # Additional files that trigger rebuilds when changed
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `libreoffice_bin` | string | `"libreoffice"` | The libreoffice executable to run |
| `formats` | string[] | `["pdf"]` | Output formats to generate (`pdf`, `pptx`) |
| `args` | string[] | `[]` | Extra arguments passed to libreoffice |
| `output_dir` | string | `"out/libreoffice"` | Base output directory |
| `extra_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |
