# RSB - Rust Build Tool Summary

A fast, incremental build tool written in Rust with template support and Python-based configuration.

## Key Features

- **Incremental builds** using SHA-256 checksums to detect changes
- **Template processing** via the Tera templating engine
- **Python configuration** - load config from `.py` files using `load_python()` function
- **CLI** built with clap

## Main Commands

- `rsb build` - Incremental build (only rebuilds changed files)
- `rsb build --force` - Force full rebuild
- `rsb clean` - Remove build artifacts and cache

## Project Structure

```
project/
├── config/           # Python config files
├── templates/        # .tera template files
└── .rsb_cache.json   # Auto-generated checksum cache
```

## How Templates Work

- Files named `templates/{X}.tera` generate output files named `{X}` in the project root
- Templates use `load_python(path="config/settings.py")` to load Python variables for templating

## Philosophy

Convention over configuration - simple naming conventions, explicit config loading, incremental builds by default.
