# Terms Processor

## Purpose

Checks that technical terms from a terms directory are backtick-quoted in Markdown
files, and provides commands to auto-fix and merge term lists across projects.

## How It Works

Loads terms from `terms/*.txt` files (one term per line, organized by category).
For each `.md` file, simulates what `rsconstruct terms fix` would produce. If the
result differs from the current content, the product fails.

The processor skips YAML frontmatter and fenced code blocks. Terms are matched
case-insensitively with word-boundary detection, longest-first to avoid partial
matches (e.g., "Android Studio" matches before "Android").

Auto-detected when a `terms/` directory exists and `.md` files are present.

## Source Files

- Input: `**/*.md`
- Output: none (checker)

## Configuration

```toml
[processor.terms]
terms_dir = "terms"       # Directory containing term list .txt files
batch = true              # Enable batch execution
extra_inputs = []         # Additional files that trigger rebuilds when changed
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `terms_dir` | string | `"terms"` | Directory containing `.txt` term list files |
| `batch` | bool | `true` | Enable batch execution |
| `extra_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

The tool accepts multiple files on the command line. When batching is enabled (default), rsconstruct passes all files in a single invocation for better performance.

## Term List Format

Each `.txt` file in the terms directory contains one term per line. Files are
typically organized by category:

```
terms/
  programming_languages.txt
  frameworks_and_libraries.txt
  databases_and_storage.txt
  devops_and_cicd.txt
  ...
```

Example `programming_languages.txt`:
```
Python
JavaScript
TypeScript
Rust
C++
Go
```

## Commands

### `rsconstruct terms fix`

Add backticks around unquoted terms in all markdown files.

```bash
rsconstruct terms fix
rsconstruct terms fix --remove-non-terms   # also remove backticks from non-terms
```

The fix is idempotent: running it twice produces the same result.

### `rsconstruct terms merge <path>`

Merge terms from another project's terms directory into the current one.
For matching filenames, new terms are added (union). Missing files are
copied in both directions.

```bash
rsconstruct terms merge ../other-project/terms
```
