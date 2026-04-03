# Json Schema Processor

## Purpose

Validates JSON schema files by checking that every object's `propertyOrdering`
array exactly matches its `properties` keys.

## How It Works

Discovers `.json` files in the project (excluding common build tool
directories), parses each as JSON, and recursively walks the structure. At every
object node with `"type": "object"`, if both `properties` and
`propertyOrdering` exist, it verifies that the two key sets match exactly.

Mismatches (keys missing from `propertyOrdering` or extra keys in
`propertyOrdering`) are reported with their JSON path. Files that contain no
`propertyOrdering` at all pass silently.

This is a pure-Rust checker — no external tool is required.

## Source Files

- Input: `**/*.json`
- Output: none (checker)

## Configuration

```toml
[processor.json_schema]
args = []                                    # Reserved for future use
extra_inputs = []                            # Additional files that trigger rebuilds when changed
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `args` | string[] | `[]` | Reserved for future use |
| `extra_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

The tool processes one file at a time. Each file is checked in a separate invocation.
