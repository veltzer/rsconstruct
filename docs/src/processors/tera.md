# Tera Processor

## Purpose

Renders Tera template files into output files, with support for loading
configuration variables from Python or Lua files.

## How It Works

Files matching configured extensions in `tera.templates/` are rendered and written
to the project root with the extension stripped:

```
tera.templates/app.config.tera  →  app.config
tera.templates/sub/readme.txt.tera  →  sub/readme.txt
```

Templates use the [Tera](https://keats.github.io/tera/) templating engine and can call
`load_python(path="...")` or `load_lua(path="...")` to load variables from config files.

### Loading Lua config

```jinja2
{% set config = load_lua(path="config/settings.lua") %}
[app]
name = "{{ config.project_name }}"
version = "{{ config.version }}"
```

Lua configs are executed via the embedded Lua 5.4 interpreter (no external
dependency). All user-defined globals (strings, numbers, booleans, tables) are
exported. Built-in Lua globals and functions are automatically filtered out.
`dofile()` and `require()` work relative to the config file's directory.

### Loading Python config

```jinja2
{% set config = load_python(path="config/settings.py") %}
[app]
name = "{{ config.project_name }}"
version = "{{ config.version }}"
```

## Source Files

- Input: `tera.templates/**/*{extensions}`
- Output: project root, mirroring the template path with the extension removed

## Configuration

```toml
[processor.tera]
strict = true                              # Fail on undefined variables (default: true)
extensions = [".tera"]                     # File extensions to process (default: [".tera"])
trim_blocks = false                        # Remove newline after block tags (default: false)
extra_inputs = ["config/settings.py"]      # Additional files that trigger rebuilds when changed
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `strict` | bool | `true` | Fail on undefined tera variables |
| `extensions` | string[] | `[".tera"]` | File extensions to discover |
| `trim_blocks` | bool | `false` | Remove first newline after block tags |
| `extra_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

Each input file is processed individually, producing its own output file.
