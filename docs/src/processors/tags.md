# Tags Processor

## Purpose

Extracts YAML frontmatter tags from markdown files into a searchable database.

## How It Works

Scans `.md` files for YAML frontmatter blocks (delimited by `---`), parses tag
metadata, and builds a tags database. The database enables querying files by
tags via `rsbuild tags` subcommands.

Optionally validates tags against a `.tags` allowlist file.

## Source Files

- Input: `**/*.md`
- Output: `out/tags/tags.db`

## Configuration

```toml
[processor.tags]
output = "out/tags/tags.db"            # Output database path
tags_file = ".tags"                    # Allowlist file for tag validation
tags_file_strict = false               # When true, missing .tags file is an error
extra_inputs = []                      # Additional files that trigger rebuilds when changed
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `output` | string | `"out/tags/tags.db"` | Path to the tags database file |
| `tags_file` | string | `".tags"` | Path to the tag allowlist file |
| `tags_file_strict` | bool | `false` | Fail if the `.tags` file is missing |
| `extra_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |
