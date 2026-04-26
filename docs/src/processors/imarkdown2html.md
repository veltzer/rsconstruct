# Imarkdown2html Processor

## Purpose

Converts Markdown files to HTML using the `pulldown-cmark` Rust crate. Native (in-process, no external tools required).

This is the native equivalent of [markdown2html](markdown2html.md), which uses the external `markdown` Perl script.

## Source Files

- Input: `**/*.md`
- Output: `out/imarkdown2html/{relative_path}.html`

## Configuration

```toml
[processor.imarkdown2html]
src_dirs = ["docs"]
output_dir = "out/imarkdown2html"    # Output directory (default)
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `output_dir` | string | `"out/imarkdown2html"` | Output directory for HTML files |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch Support

Each input file is processed individually, producing its own output file.

## Clean behavior

This processor is a Generator — `rsconstruct clean outputs` removes each declared output file individually with no directory recursion. After all per-product cleans complete, the orchestrator removes any parent directories that are now empty. Pass `--no-empty-dirs` to keep them. See [Clean behavior](../processors.md#clean-behavior) and [`rsconstruct clean`](../commands.md#rsconstruct-clean).
