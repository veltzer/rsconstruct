# Template Processor

## Purpose

Renders Tera template files into output files, with support for loading Python
configuration variables.

## How It Works

Files matching configured extensions in `templates/` are rendered and written
to the project root with the extension stripped:

```
templates/app.config.tera  →  app.config
templates/sub/readme.txt.tera  →  sub/readme.txt
```

Templates use the [Tera](https://keats.github.io/tera/) templating engine and can call
`load_python(path="...")` to load variables from Python `.py` files. The Python files are
parsed for simple assignments (strings, numbers, booleans, lists).

### Loading Python config

```jinja2
{% set config = load_python(path="config/settings.py") %}
[app]
name = "{{ config.project_name }}"
version = "{{ config.version }}"
```

## Source Files

- Input: `templates/**/*{extensions}`
- Output: project root, mirroring the template path with the extension removed

## Configuration

```toml
[processor.template]
strict = true                              # Fail on undefined variables (default: true)
extensions = [".tera"]                     # File extensions to process (default: [".tera"])
trim_blocks = false                        # Remove newline after block tags (default: false)
extra_inputs = ["config/settings.py"]      # Additional files that trigger rebuilds when changed
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `strict` | bool | `true` | Fail on undefined template variables |
| `extensions` | string[] | `[".tera"]` | File extensions to discover |
| `trim_blocks` | bool | `false` | Remove first newline after block tags |
| `extra_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |
