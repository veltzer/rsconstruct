# Tags Processor

## Purpose

Extracts YAML frontmatter tags from markdown files into a searchable database.

## How It Works

Scans `.md` files for YAML frontmatter blocks (delimited by `---`), parses tag
metadata, and builds a [redb](https://github.com/cberner/redb) database. The
database enables querying files by tags via `rsconstruct tags` subcommands.

Validates tags against a `tags_dir` directory containing tag list files.

### Tag Indexing

Two kinds of frontmatter fields are indexed:

- **List fields** — each item becomes a bare tag.
  ```yaml
  tags:
    - docker
    - python
  ```
  Produces tags: `docker`, `python`.

- **Scalar fields** — indexed as `key:value` (colon separator).
  ```yaml
  level: beginner
  difficulty: 3
  published: true
  url: https://example.com/path
  ```
  Produces tags: `level:beginner`, `difficulty:3`, `published:true`,
  `url:https://example.com/path`.

Both inline YAML lists (`tags: [a, b, c]`) and multi-line lists are supported.

### The `tags_dir` Allowlist

The `tags_dir` directory (default: `tag_lists/`) contains `.txt` files that
define the allowed tags. Each file `<name>.txt` contributes tags as
`<name>:<line>` pairs. For example:

```
tag_lists/
├── level.txt        # Contains: beginner, intermediate, advanced
├── languages.txt    # Contains: python, rust, go, ...
└── tools.txt        # Contains: docker, ansible, ...
```

`level.txt` with content `beginner` produces the allowed tag `level:beginner`.

Unknown tags cause a build error with typo suggestions (Levenshtein distance).
The tags processor is only auto-detected when `tags_dir` contains `.txt` files.

## Source Files

- Input: `**/*.md`
- Output: `out/tags/tags.db`

## Configuration

```toml
[processor.tags]
output = "out/tags/tags.db"            # Output database path
tags_dir = "tag_lists"                 # Directory containing tag list files
extra_inputs = []                      # Additional files that trigger rebuilds when changed
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `output` | string | `"out/tags/tags.db"` | Path to the tags database file |
| `tags_dir` | string | `"tag_lists"` | Directory containing `.txt` tag list files |
| `extra_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Subcommands

All subcommands require a prior `rsconstruct build` to populate the database.
All support `--json` for machine-readable output.

### Querying

| Command | Description |
|---------|-------------|
| `rsconstruct tags list` | List all unique tags (sorted) |
| `rsconstruct tags files TAG [TAG...]` | List files matching all given tags (AND) |
| `rsconstruct tags files --or TAG [TAG...]` | List files matching any given tag (OR) |
| `rsconstruct tags grep TEXT` | Search for tags containing a substring |
| `rsconstruct tags grep -i TEXT` | Case-insensitive tag search |
| `rsconstruct tags for-file PATH` | List all tags for a specific file (supports suffix matching) |
| `rsconstruct tags frontmatter PATH` | Show raw parsed frontmatter for a file |
| `rsconstruct tags count` | Show each tag with its file count, sorted by frequency |
| `rsconstruct tags tree` | Show tags grouped by key (e.g. `level=` group) vs bare tags |
| `rsconstruct tags stats` | Show database statistics (file count, unique tags, associations) |

### Validation

| Command | Description |
|---------|-------------|
| `rsconstruct tags unused` | List tags in `tags_dir` that no file uses |
| `rsconstruct tags unused --strict` | Same, but exit with error if any unused tags exist (for CI) |
| `rsconstruct tags validate` | Validate indexed tags against `tags_dir` without rebuilding |
