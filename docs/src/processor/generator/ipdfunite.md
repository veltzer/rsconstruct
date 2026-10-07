# Ipdfunite Processor

## Purpose

Merges PDFs from subdirectories into one PDF per subdirectory. Native (in-process, using [lopdf](https://github.com/J-F-Liu/lopdf); no external tools required). An in-process alternative to [pdfunite](pdfunite.md), with the same configuration apart from `command` and `args`.

## How It Works

Built for course/module workflows, where the slide decks of a course live in one directory and an upstream processor (usually [marp](marp.md)) renders each deck to PDF:

1. Every file with extension `source_ext` under `source_dir` is found and grouped by its directory.
2. For each directory, the PDF the upstream processor made from each file is located under `source_output_dir`, at the same relative path with a `.pdf` extension.
3. Those PDFs are merged, in file-name order, into `<output_dir>/<directory>.pdf`.

```text
marp/courses/rust/01-intro.md  ->  out/processor.generator.marp/courses/rust/01-intro.pdf  ┐
marp/courses/rust/02-types.md  ->  out/processor.generator.marp/courses/rust/02-types.pdf  ┴> out/processor.generator.ipdfunite/rust.pdf
```

The input PDFs are declared as the product's inputs, so the merge runs after the upstream processor has produced them and reruns when any of them changes.

Bookmarks (outlines) of the input PDFs are not carried over to the merged PDF.

## Source Files

- Input: PDFs under `source_output_dir` that correspond to the `source_ext` files under `source_dir`
- Output: `out/processor.generator.ipdfunite/<directory>.pdf`, one per source directory

## Configuration

```toml
[processor.generator.ipdfunite]
source_dir = "marp/courses"                         # Directory containing course subdirectories
source_ext = ".md"                                  # Extension of the source files in them
source_output_dir = "out/processor.generator.marp"  # Where the upstream processor put the PDFs
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `source_dir` | string | `"marp/courses"` | Directory containing course subdirectories |
| `source_ext` | string | `".md"` | Extension of the source files to look for |
| `source_output_dir` | string | `"out/processor.generator.marp"` | Directory where the upstream processor writes the PDFs |
| `output_dir` | string | `"out/processor.generator.ipdfunite"` | Output directory for merged PDFs |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

Each directory is merged individually, producing its own output file. Merges run one at a time (`max_jobs` is capped at 1).

## Clean behavior

This processor is a Generator — `rsconstruct clean outputs` removes each declared output file individually with no directory recursion. After all per-product cleans complete, the orchestrator removes any parent directories that are now empty. Pass `--no-empty-dirs` to keep them. See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
