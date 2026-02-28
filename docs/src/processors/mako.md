# Mako Processor

## Purpose

Renders Mako template files into output files using the Python Mako template library.

## How It Works

Files matching configured extensions in `templates/` are rendered via `python3` using
the `mako` Python library. Output is written with the extension stripped and the
`templates/` prefix removed:

```
templates/app.config.mako  →  app.config
templates/sub/readme.txt.mako  →  sub/readme.txt
```

Templates use the [Mako](https://www.makotemplates.org/) templating engine. A
`TemplateLookup` is configured with the project root as the lookup directory, so
templates can include or inherit from other templates using relative paths.

## Source Files

- Input: `templates/**/*{extensions}`
- Output: project root, mirroring the template path (minus `templates/` prefix) with the extension removed

## Configuration

```toml
[processor.mako]
extensions = [".mako"]                    # File extensions to process (default: [".mako"])
extra_inputs = ["config/settings.py"]     # Additional files that trigger rebuilds when changed
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `extensions` | string[] | `[".mako"]` | File extensions to discover |
| `extra_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |
