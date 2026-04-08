# Yaml2json Processor

## Purpose

Converts YAML files to JSON. Native (in-process, no external tools required).

## How It Works

Discovers YAML files in the configured directories and converts each to a pretty-printed JSON file.

## Source Files

- Input: `**/*.yml`, `**/*.yaml`
- Output: `out/yaml2json/{relative_path}.json`

## Configuration

```toml
[processor.yaml2json]
scan_dirs = ["yaml"]
output_dir = "out/yaml2json"    # Output directory (default)
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `output_dir` | string | `"out/yaml2json"` | Output directory for JSON files |
| `extra_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch Support

Each input file is processed individually, producing its own output file.
