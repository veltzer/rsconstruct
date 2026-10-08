# Isass Processor

## Purpose

Compiles Sass and SCSS files to CSS. Native (in-process, using the [grass](https://github.com/connorskees/grass) compiler; no external tools required). An in-process alternative to [sass](sass.md).

## How It Works

Each `.scss` or `.sass` file under `src_dirs` is compiled to a `.css` file under `output_dir`, keeping its path relative to the source directory:

```text
sass/style.scss             ->  out/processor.generator.isass/style.css
sass/components/button.scss ->  out/processor.generator.isass/components/button.css
```

`@use` and `@import` resolve relative to the importing file. Compile errors fail the product with grass's message.

**Declare the [sass analyzer](../../analyzers/sass.md) alongside this processor.** It makes every file a stylesheet loads (`@use`, `@forward`, `@import`, transitively) an input of that stylesheet. Without it, a product's only input is its own source file: editing a partial does **not** rebuild the stylesheets that use it, and they keep their old CSS.

Like the `sass` command line, **partials are not compiled**: a file whose name starts with `_` (`_vars.scss`) exists to be `@use`d by other stylesheets and gets no `.css` of its own. Set `skip_partials = false` to compile every matching file, partials included.

## Source Files

- Input: `.scss` and `.sass` files under `src_dirs`
- Output: `out/processor.generator.isass/` mirroring the source structure, with `.css` extension

The default `src_dirs` is empty, so a stanza with no `src_dirs` or `src_files` compiles nothing.

## Configuration

```toml
[processor.generator.isass]
src_dirs = ["sass"]

[analyzer.sass]
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `src_dirs` | string[] | `[]` | Directories to scan |
| `src_extensions` | string[] | `[".scss", ".sass"]` | File extensions to compile |
| `output_dir` | string | `"out/processor.generator.isass"` | Output directory |
| `skip_partials` | bool | `true` | Leave files whose name starts with `_` (partials) out of discovery, as the `sass` CLI does. `false` compiles every matching file |
| `dep_inputs` | string[] | `[]` | Extra files whose changes rebuild every product |

## Batch support

Each input file is processed individually, producing its own output file.

## Clean behavior

This processor is a Generator — `rsconstruct clean outputs` removes each declared output file individually with no directory recursion. After all per-product cleans complete, the orchestrator removes any parent directories that are now empty. Pass `--no-empty-dirs` to keep them. See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
