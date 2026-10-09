# Ijinja2 Processor

## Purpose

Renders Jinja2 templates into output files in-process, with [minijinja](https://github.com/mitsuhiko/minijinja) (no Python, no `jinja2` package). The Rust alternative to the [jinja2](jinja2.md) generator, with the same contract: the same files are found, written to the same places, rendered with the same context.

## How It Works

Files under `src_dirs` whose names end in a `src_extensions` entry are rendered, and the output is written to the same relative path with the extension removed:

```text
templates.jinja2/app.config.j2      →  app.config
templates.jinja2/sub/readme.txt.j2  →  sub/readme.txt
```

The project root is the template root, so `{% include %}`, `{% extends %}` and `{% import %}` take paths relative to it (`{% include 'templates.jinja2/parts/header.inc' %}`). The environment variables are the context: `{{ HOME }}` is the home directory, `{{ MISSING }}` renders as nothing and `{{ MISSING | default('x') }}` as `x`. The engine is set up like a plain Jinja2 `Environment`: no autoescaping (not even for `.html` outputs), the single trailing newline of a template dropped, blocks and lines trimmed only where `-` asks for it.

### What minijinja renders differently

minijinja implements Jinja2's syntax, its filters and tests, and (through minijinja-contrib) the Python string and dict methods templates call, such as `.items()`, `.lower()`, `.startswith()`, `.format()`. Where minijinja's version of a filter takes fewer arguments than Jinja2's, ijinja2 supplies Jinja2's: `truncate(length, killwords, end)`, `replace(old, new, count)`, `round(precision, method)`, `int(default, base)`, `sum(attribute, start)`, `center(width)`, `forceescape`. Checked against Python Jinja2 over 34 templates and 59 single-construct probes covering control flow, loop variables, `set` and `namespace`, macros with `call`, `include`/`extends`/`super()`, whitespace control, comments and `raw`, dictionaries and slicing, the string methods, the tests, and the filters: identical output except for the following, each a construct rather than a quirk of the data:

- `wordwrap` takes its width as a keyword (`wordwrap(width=5)`), and `truncate`'s `leeway` is keyword-only.
- `sum` adds numbers only (`sum(start=[])` to concatenate lists is an error).
- `2 ** -1` is an error (Python gives `0.5`).
- A dictionary key that is also a Python method name (`items`, `keys`, `get`) is read as the key: `{{ d.items }}` is the value where Jinja2 prints a bound method. Likewise the `attr` filter reads keys, where Jinja2 only reads Python attributes.
- `[1, 2] | reverse` prints the reversed list where Jinja2 prints an iterator object.
- `urlencode` on a dictionary encodes a space as `%20`, Jinja2 as `+`.

A template that hits one of the first three fails with a message naming the template and the construct; it stays on the `jinja2` generator or is rewritten.

## Source Files

- Input: `.j2` files under `src_dirs`
- Output: project root, mirroring the template path (minus the `src_dirs` prefix) with the extension removed

The default `src_dirs` is empty, so a stanza with no `src_dirs` or `src_files` renders nothing: name the template directories.

## Configuration

```toml
[processor.generator.ijinja2]
src_dirs = ["templates.jinja2"]
dep_inputs = ["templates.jinja2/parts/header.inc"]   # included files are not tracked automatically
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `src_dirs` | string[] | `[]` | Directories to scan |
| `src_extensions` | string[] | `[".j2"]` | File extensions to render |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds (included and extended templates) |

Switching a repo from jinja2: `[processor.generator.jinja2]` → `[processor.generator.ijinja2]`; `command` (the Python interpreter) goes away.

## Batch support

Each template is rendered individually, producing its own output file.

## Clean behavior

This processor is a Generator — `rsconstruct clean outputs` removes each declared output file individually with no directory recursion. After all per-product cleans complete, the orchestrator removes any parent directories that are now empty. Pass `--no-empty-dirs` to keep them. See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
