# Mdbook Processor

## Purpose

Builds [mdbook](https://rust-lang.github.io/mdBook/) documentation projects.

## How It Works

Discovers `book.toml` files indicating mdbook projects, collects sibling `.md`
and `.toml` files as inputs, and runs `mdbook build`. A non-zero exit code
fails the product.

## Source Files

- Input: `**/book.toml` (plus sibling `.md`, `.toml` files)
- Output: none (creator — produces output in `book` directory)

## Configuration

```toml
[processor.mdbook]
command = "mdbook"                     # The mdbook command to run
output_dir = "book"                    # Output directory for generated docs
args = []                              # Additional arguments to pass to mdbook
dep_inputs = []                      # Additional files that trigger rebuilds when changed
cache_output_dir = true                # Cache the output directory for fast restore after clean
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `command` | string | `"mdbook"` | The mdbook executable to run |
| `output_dir` | string | `"book"` | Output directory for generated documentation |
| `args` | string[] | `[]` | Extra arguments passed to mdbook |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |
| `cache_output_dir` | boolean | `true` | Cache the `book/` directory so `rsconstruct clean && rsconstruct build` restores from cache |

## Batch support

Runs as a single whole-project operation (e.g., `cargo build`, `npm install`).

## Clean behavior

This processor is a Creator — `rsconstruct clean outputs` removes its declared `output_dirs` recursively (the build tool produces an unknown set of files inside, so directory-level deletion is the only option). After all per-product cleans complete, the orchestrator removes any parent directories that are now empty. Pass `--no-empty-dirs` to keep them. See [Clean behavior](../processors.md#clean-behavior) and [`rsconstruct clean`](../commands.md#rsconstruct-clean).
