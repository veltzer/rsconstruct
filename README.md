# RSB - Rust Build Tool

A fast, incremental build tool written in Rust with template support and Python-based configuration.

## Features

- **Incremental Builds**: Uses SHA-256 checksums to detect file changes and only rebuild what's necessary
- **Template Processing**: Uses Tera templating engine for generating files from templates
- **Python Configuration**: Load configuration from Python files via the `load_python()` Tera function
- **Simple CLI**: Clean and intuitive command-line interface built with clap
- **Convention over Configuration**: Simple naming convention for templates

## Installation

```bash
cargo build --release
```

## Usage

### Build Command
Executes an incremental build, processing templates only if they've changed:

```bash
rsb build
```

Force a full rebuild:

```bash
rsb build --force
```

### Clean Command
Remove all build artifacts and cache files:

```bash
rsb clean
```

## Project Structure

```
project/
├── config/              # Python configuration files (convention)
│   └── *.py            # Python files with configuration variables
├── templates/           # Template files
│   └── {name}.tera     # Each .tera file creates an output file named {name}
└── .rsb_cache.json     # Cache file for tracking checksums (auto-generated)
```

## How It Works

1. **Template Processing**: RSB scans the `templates/` directory for `.tera` files
2. **Configuration Loading**: Templates use the `load_python()` function to load Python config files
3. **File Generation**: Each `templates/{name}.tera` file generates a file named `{name}` in the project root
4. **Incremental Building**: Uses checksums to skip unchanged templates

## Template Function: load_python()

Templates can load Python configuration files using the built-in `load_python()` function:

```jinja2
{% set config = load_python(path="config/settings.py") %}
```

This executes the Python file and makes all its variables available in the template.

## Example

1. Create a configuration file `config/settings.py`:
```python
project_name = "MyProject"
version = "1.0.0"
debug_mode = True
optimization_level = 2
```

2. Create a template `templates/app.conf.tera`:
```jinja2
{% set config = load_python(path="config/settings.py") %}
[app]
name = "{{ config.project_name }}"
version = "{{ config.version }}"
debug = {{ config.debug_mode }}
optimization = {{ config.optimization_level }}
```

3. Run the build:
```bash
rsb build
```

This generates a file `app.conf` in your project root with the rendered template.

## Design Philosophy

RSB follows the principle of "convention over configuration":
- Templates named `{X}.tera` automatically generate files named `{X}`
- Configuration loading is explicit via the `load_python()` function
- Incremental builds are the default behavior
- No complex configuration files needed
