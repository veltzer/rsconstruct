# Jinja2 Processor

## Purpose

Renders Jinja2 template files into output files using the Python Jinja2 template library.

The Rust alternative is [ijinja2](ijinja2.md): the same discovery, output paths and environment-variable context, rendered in-process with minijinja. Switching is `jinja2` → `ijinja2` in the stanza header. A template that leans on Python-only calls (`'{}'.format()`, `%`-formatting, `pprint`) stays here.

## How It Works

Files matching configured extensions in `templates.jinja2/` are rendered via `python3` using
the `jinja2` Python library. Output is written with the extension stripped and the
`templates.jinja2/` prefix removed:

```text
templates.jinja2/app.config.j2  →  app.config
templates.jinja2/sub/readme.txt.j2  →  sub/readme.txt
```

Templates use the [Jinja2](https://jinja.palletsprojects.com/) templating engine. A
`FileSystemLoader` is configured with the project root as the search directory, so
templates can include or extend other templates using relative paths. Environment
variables are passed to the template context.

## Source Files

- Input: `templates.jinja2/**/*{src_extensions}`
- Output: project root, mirroring the template path (minus `templates.jinja2/` prefix) with the extension removed

## Configuration

```toml
[processor.generator.jinja2]
src_extensions = [".j2"]                      # File extensions to process (default: [".j2"])
dep_inputs = ["config/settings.py"]     # Additional files that trigger rebuilds when changed
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `src_extensions` | string[] | `[".j2"]` | File extensions to discover |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

Each input file is processed individually, producing its own output file.

## Clean behavior

This processor is a Generator — `rsconstruct clean outputs` removes each declared output file individually with no directory recursion. After all per-product cleans complete, the orchestrator removes any parent directories that are now empty. Pass `--no-empty-dirs` to keep them. See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
