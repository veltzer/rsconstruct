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
src_dirs = ["yaml"]
output_dir = "out/yaml2json"    # Output directory (default)
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `output_dir` | string | `"out/yaml2json"` | Output directory for JSON files |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch Support

Each input file is processed individually, producing its own output file.

## Clean behavior

This processor is a Generator — `rsconstruct clean outputs` removes each declared output file individually with no directory recursion. After all per-product cleans complete, the orchestrator removes any parent directories that are now empty. Pass `--no-empty-dirs` to keep them. See [Clean behavior](../processors.md#clean-behavior) and [`rsconstruct clean`](../commands.md#rsconstruct-clean).
