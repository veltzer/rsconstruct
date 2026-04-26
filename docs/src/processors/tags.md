# Tags Processor

## Purpose

Extracts YAML frontmatter tags from markdown files into a searchable database
with comprehensive validation.

## How It Works

Scans `.md` files for YAML frontmatter blocks (delimited by `---`), parses tag
metadata, and builds a [redb](https://github.com/cberner/redb) database. The
database enables querying files by tags via `rsconstruct tags` subcommands.

### Tag Indexing

Two kinds of frontmatter fields are indexed:

- **List fields** — each item becomes a tag as-is.
  ```yaml
  tags:
    - tools:docker
    - tools:python
  ```
  Produces tags: `tools:docker`, `tools:python`.

- **Scalar fields** — indexed as `key:value` (colon separator).
  ```yaml
  level: beginner
  category: big-data
  duration_hours: 24
  ```
  Produces tags: `level:beginner`, `category:big-data`, `duration_hours:24`.

Both inline YAML lists (`tags: [a, b, c]`) and multi-line lists are supported.

### The `tags_dir` Allowlist

The `tags_dir` directory (default: `tags/`) contains `.txt` files that
define the allowed tags. Each file `<name>.txt` contributes tags as
`<name>:<line>` pairs. For example:

```
tags/
├── level.txt        # Contains: beginner, intermediate, advanced
├── languages.txt    # Contains: python, rust, go, ...
├── tools.txt        # Contains: docker, ansible, ...
└── audiences.txt    # Contains: developers, architects, ...
```

`level.txt` with content `beginner` produces the allowed tag `level:beginner`.

The tags processor is only auto-detected when `tags_dir` contains `.txt` files.

## Build-Time Validation

During every build, the tags processor runs the following checks. Any failure
stops the build with a descriptive error message.

### Required Frontmatter Fields

When `required_fields` is configured, every `.md` file must contain those
frontmatter fields. Empty lists (`[]`) and empty strings are treated as missing.
Files with no frontmatter block at all also fail:

```toml
[processor.tags]
required_fields = ["tags", "level", "category", "duration_hours", "audiences"]
```

```
Missing required frontmatter fields:
  syllabi/courses/intro.md: category, duration_hours
  syllabi/courses/advanced.md: audiences
```

### Required Field Groups

When `required_field_groups` is configured, every file must satisfy **at least
one** group (all fields in that group present). This handles cases where files
may have alternative sets of fields:

```toml
[processor.tags]
required_field_groups = [
    ["duration_hours"],
    ["duration_hours_long", "duration_hours_short"],
]
```

A file with `duration_hours` passes. A file with both `duration_hours_long` and
`duration_hours_short` passes. A file with only `duration_hours_short` (partial
group) or none of these fields fails:

```
Files missing required field groups (must satisfy at least one):
  syllabi/courses/intro.md: none of [duration_hours] or [duration_hours_long, duration_hours_short]
```

### Required Values

When `required_values` is configured, scalar fields must contain a value that
exists in the corresponding `tags/<field>.txt` file. This catches typos in
scalar values:

```toml
[processor.tags]
required_values = ["level", "category"]
```

```
Invalid values for validated fields:
  syllabi/courses/intro.md: level=begginer (not in tags/level.txt)
```

### Field Types

When `field_types` is configured, frontmatter fields must have the expected
type. Supported types: `"list"`, `"scalar"`, `"number"`.

```toml
[processor.tags.field_types]
tags = "list"
level = "scalar"
duration_hours = "number"
```

```
Field type mismatches:
  syllabi/courses/intro.md: 'level' expected list, got scalar
```

### Unique Fields

When `unique_fields` is configured, no two files may share the same value for
that field:

```toml
[processor.tags]
unique_fields = ["title"]
```

```
Duplicate values for unique fields:
  title='Intro to Docker' in:
    - syllabi/courses/docker_intro.md
    - syllabi/courses/containers/docker_intro.md
```

### Sorted Tags

When `sorted_tags = true`, list-type frontmatter fields must have their items
in lexicographic sorted order. This reduces diff noise in version control:

```toml
[processor.tags]
sorted_tags = true
```

```
List tags are not in sorted order:
  syllabi/courses/intro.md field 'tags': 'tools:alpha' should come after 'tools:beta'
```

### Duplicate Tags Within a File

The same tag cannot appear twice in a single file's frontmatter:

```
Duplicate tags found within files:
  tools:docker in syllabi/courses/containers/intro.md
```

### Duplicate Tags Across Tag Lists

The same `category:value` tag cannot be defined in multiple `tags_dir/*.txt`
files. Note that the same value in different categories is fine (`tools:docker`
and `infra:docker` are distinct tags):

```
Duplicate tags found across tags files:
  tools:docker in tools.txt and infra.txt
```

### Unknown Tags

Every tag found in frontmatter must exist in `tags_dir`. Unknown tags cause an
error with a typo suggestion (Levenshtein distance):

```
Unknown tags found (not in tags):
  tools:dockker (did you mean 'tools:docker'?)
    - syllabi/courses/containers/intro.md
```

### Unused Tags

Every tag defined in `tags_dir/*.txt` must be used by at least one `.md` file.
This catches stale entries that should be cleaned up:

```
Unused tags in tags (not used by any file):
  tools:vagrant
  languages:fortran
```

## Source Files

- Input: `**/*.md` (configurable via `src_dirs` / `src_extensions`)
- Output: `out/tags/tags.db`

## Configuration

```toml
[processor.tags]
output = "out/tags/tags.db"                                       # Output database path
tags_dir = "tags"                                            # Directory containing tag list files
required_fields = ["tags", "level", "category"]                   # Fields every .md file must have
required_field_groups = [                                         # At least one group must be fully present
    ["duration_hours"],
    ["duration_hours_long", "duration_hours_short"],
]
required_values = ["level", "category"]                           # Scalar fields validated against tags
unique_fields = ["title"]                                         # Fields that must be unique across files
sorted_tags = true                                                # Require list items in sorted order
dep_inputs = []                                                 # Additional files that trigger rebuilds

[processor.tags.field_types]
tags = "list"                                                     # Must be a YAML list
level = "scalar"                                                  # Must be a string
duration_hours = "number"                                         # Must be numeric
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `output` | string | `"out/tags/tags.db"` | Path to the tags database file |
| `tags_dir` | string | `"tags"` | Directory containing `.txt` tag list files |
| `required_fields` | string[] | `[]` | Frontmatter fields that every `.md` file must have |
| `required_field_groups` | string[][] | `[]` | Alternative field groups; at least one group must be fully present |
| `required_values` | string[] | `[]` | Scalar fields whose values must exist in `tags/<field>.txt` |
| `unique_fields` | string[] | `[]` | Fields whose values must be unique across all files |
| `field_types` | map | `{}` | Expected types per field: `"list"`, `"scalar"`, or `"number"` |
| `sorted_tags` | bool | `false` | Require list items in sorted order within each file |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

Each input file is processed individually, producing its own output file.

## Subcommands

All subcommands require a prior `rsconstruct build` to populate the database
(except `check` which reads files directly).
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

### Reporting

| Command | Description |
|---------|-------------|
| `rsconstruct tags matrix` | Show a coverage matrix of tag categories per file |
| `rsconstruct tags coverage` | Show percentage of files that have each tag category |
| `rsconstruct tags orphans` | Find files with no tags at all |
| `rsconstruct tags suggest PATH` | Suggest tags for a file based on similarity to other tagged files |

### Validation

| Command | Description |
|---------|-------------|
| `rsconstruct tags check` | Run all validations without building (fast lint pass) |
| `rsconstruct tags unused` | List tags in `tags_dir` that no file uses |
| `rsconstruct tags unused --strict` | Same, but exit with error if any unused tags exist (for CI) |
| `rsconstruct tags validate` | Validate indexed tags against `tags_dir` without rebuilding |

## Clean behavior

This processor is a Generator — `rsconstruct clean outputs` removes each declared output file individually with no directory recursion. After all per-product cleans complete, the orchestrator removes any parent directories that are now empty. Pass `--no-empty-dirs` to keep them. See [Clean behavior](../processors.md#clean-behavior) and [`rsconstruct clean`](../commands.md#rsconstruct-clean).
