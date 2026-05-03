# Terms Processor

## Purpose

Checks that technical terms from a terms directory are backtick-quoted in Markdown
files, and provides commands to auto-fix and merge term lists across projects.

## How It Works

Loads terms from `dir_terms_unambiguous/*.txt` files (one term per line, organized by category).
For each `.md` file, simulates what `rsconstruct terms fix` would produce. If the
result differs from the current content, the product fails.

The processor skips YAML frontmatter and fenced code blocks. Terms are matched
**case-sensitively** with word-boundary detection, longest-first to avoid
partial matches (e.g., "Android Studio" matches before "Android").

## Case sensitivity

All comparisons in the terms processor are case-sensitive — the term lists,
the scanner, and the disjoint check between `dir_terms_unambiguous` and
`dir_terms_ambiguous`. Terms are stored verbatim in their canonical casing
(e.g. `Docker`, `AWS`, `awk`, `/etc/fstab`) and prose must match that casing
exactly to be flagged. `Docker` in the list will match `Docker` in prose but
not `docker` or `DOCKER`. If you want to enforce multiple casings of the
same word, list each one separately.

The disjoint invariant between the two directories is also case-sensitive,
so `Docker` in `dir_terms_unambiguous` and `docker` in `dir_terms_ambiguous`
will not be reported as overlapping. (If both lists are maintained in
canonical casing this is a non-issue.)

If `dir_terms_ambiguous` exists on disk, terms in it are loaded and
validated to be **disjoint** from `dir_terms_unambiguous` — any term
appearing in both fails the build with the offending term names listed.
A missing ambiguous directory is treated as an empty list (no error), so
projects without an ambiguous list just use the unambiguous-only behavior. Ambiguous terms are
*not* required to be backticked; the directory exists so projects can track
words that look technical but have ordinary meanings (e.g. "server", "client")
without forcing them to be quoted everywhere. Both directories' `.txt` files
are tracked as build inputs, so editing either invalidates the cache.

By default (`forbid_backticked_ambiguous = true`), backticking an ambiguous
term is itself a build error: backticks falsely assert the technical
reading, so prose like `` the `server` was slow `` fails. `terms fix`
strips those backticks. Set `forbid_backticked_ambiguous = false` to
disable this check (ambiguous terms are then loaded only to validate the
disjoint invariant, and their use in backticks is neither flagged nor
modified).

Auto-detected when `dir_terms_unambiguous` exists and `.md` files are present.

## Source Files

- Input: `**/*.md`
- Output: none (checker)

## Configuration

```toml
[processor.terms]
dir_terms_unambiguous = "terms/unambiguous"   # Directory of unambiguous term lists
dir_terms_ambiguous = "terms/ambiguous"       # Optional: directory of ambiguous terms
forbid_backticked_ambiguous = true            # Backticking an ambiguous term is an error
batch = true                                  # Enable batch execution
dep_inputs = []                               # Additional files that trigger rebuilds
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `dir_terms_unambiguous` | string | `"terms/unambiguous"` | Directory containing `.txt` files of unambiguous terms. Terms here must be backticked in markdown. |
| `dir_terms_ambiguous` | string | `"terms/ambiguous"` | Directory of ambiguous terms. Treated as empty if the directory does not exist on disk. Build fails if any term overlaps with `dir_terms_unambiguous`. Terms here are **not** required to be backticked. |
| `forbid_backticked_ambiguous` | bool | `true` | If true, backticking an ambiguous term is a build error and `terms fix` strips those backticks. |
| `batch` | bool | `true` | Enable batch execution |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

The tool accepts multiple files on the command line. When batching is enabled (default), rsconstruct passes all files in a single invocation for better performance.

## Term List Format

Each `.txt` file in a terms directory contains one term per line. Files are
typically organized by category. Projects that distinguish unambiguous
from ambiguous terms use two parallel directories:

```
terms/unambiguous/
  programming_languages.txt
  frameworks_and_libraries.txt
  databases_and_storage.txt
  devops_and_cicd.txt
  ...
terms/ambiguous/
  general_technical_terms.txt
  ...
```

The two directories must be **disjoint** — the same term cannot appear in
both. The build fails (with the overlapping term names listed) if the
invariant is violated.

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

Add backticks around unquoted unambiguous terms in all markdown files.
When `forbid_backticked_ambiguous = true` (default), also strip backticks
around ambiguous terms.

```bash
rsconstruct terms fix
rsconstruct terms fix --remove-non-terms   # also remove backticks from arbitrary non-terms
```

The fix is idempotent: running it twice produces the same result.

### `rsconstruct terms merge <path>`

Merge terms from another project's terms directory into the current one.
For matching filenames, new terms are added (union). Missing files are
copied in both directions.

```bash
rsconstruct terms merge ../other-project/terms
```

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../processors.md#clean-behavior) and [`rsconstruct clean`](../commands.md#rsconstruct-clean).
