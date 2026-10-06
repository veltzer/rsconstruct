# Isass Processor

## Purpose

Compiles Sass and SCSS files to CSS. Native (in-process, using the [grass](https://github.com/connorskees/grass) compiler; no external tools required). An in-process alternative to [sass](sass.md).

## How It Works

Each `.scss` or `.sass` file under `src_dirs` is compiled to a `.css` file under `output_dir`, keeping its path relative to the source directory:

```
sass/style.scss             ->  out/processor.generator.isass/style.css
sass/components/button.scss ->  out/processor.generator.isass/components/button.css
```

`@use` and `@import` resolve relative to the importing file. Compile errors fail the product with grass's message.

Two things differ from what the `sass` command line does:

- **Partials are compiled too.** Every matching file is a product, including partials such as `_vars.scss`, which produce their own (often empty) `.css`. Keep partials out with `src_exclude_files` or `src_exclude_paths`.
- **Imported files are not tracked.** A product's inputs are its own source file only. Editing a partial that another file `@use`s does **not** rebuild the importer, which keeps its old CSS. List shared partials in `dep_inputs` so that a change to them rebuilds every file.

## Source Files

- Input: `.scss` and `.sass` files under `src_dirs`
- Output: `out/processor.generator.isass/` mirroring the source structure, with `.css` extension

The default `src_dirs` is empty, so a stanza with no `src_dirs` or `src_files` compiles nothing.

## Configuration

```toml
[processor.generator.isass]
src_dirs = ["sass"]
src_exclude_files = ["_vars.scss", "_mixins.scss"]
dep_inputs = ["sass/_vars.scss", "sass/_mixins.scss"]
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `src_dirs` | string[] | `[]` | Directories to scan |
| `src_extensions` | string[] | `[".scss", ".sass"]` | File extensions to compile |
| `output_dir` | string | `"out/processor.generator.isass"` | Output directory |
| `dep_inputs` | string[] | `[]` | Extra files whose changes rebuild every product (list imported partials here) |

## Batch support

Each input file is processed individually, producing its own output file.

## Clean behavior

This processor is a Generator — `rsconstruct clean outputs` removes each declared output file individually with no directory recursion. After all per-product cleans complete, the orchestrator removes any parent directories that are now empty. Pass `--no-empty-dirs` to keep them. See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
