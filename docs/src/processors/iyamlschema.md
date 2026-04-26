# Iyamlschema Processor

## Purpose

Validates YAML files against JSON schemas referenced by a `$schema` URL field in each file. Checks both schema conformance and property ordering. Native (in-process, no external tools required).

## How It Works

For each YAML file:
1. Parses the YAML content
2. Reads the `$schema` field to get the schema URL
3. Fetches the schema (cached in `.rsconstruct/webcache.redb`)
4. Validates the data against the schema (including resolving remote `$ref` references)
5. Checks that object keys appear in the order specified by `propertyOrdering` fields in the schema

Fails if any file is missing `$schema`, fails schema validation, or has keys in the wrong order.

## Configuration

```toml
[processor.iyamlschema]
src_dirs = ["yaml"]
check_ordering = true    # Check propertyOrdering (default: true)
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `check_ordering` | boolean | `true` | Whether to check property ordering against `propertyOrdering` in the schema |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Schema Requirements

Each YAML file must contain a `$schema` field with a URL pointing to a JSON schema:

```yaml
$schema: "https://example.com/schemas/mydata.json"
name: Alice
age: 30
```

The schema is fetched via HTTP and cached locally. Subsequent builds use the cached version. Use `rsconstruct webcache clear` to force re-fetching.

## Property Ordering

If the schema contains `propertyOrdering` arrays, the processor checks that data keys appear in the specified order:

```json
{
  "type": "object",
  "properties": {
    "name": { "type": "string" },
    "age": { "type": "integer" }
  },
  "propertyOrdering": ["name", "age"]
}
```

Set `check_ordering = false` to disable this check.

## Batch Support

Files are validated individually within a batch. Partial failure is handled correctly.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../processors.md#clean-behavior) and [`rsconstruct clean`](../commands.md#rsconstruct-clean).
