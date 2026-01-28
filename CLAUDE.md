# RSB - Rust Build Tool Summary

A fast, incremental build tool written in Rust with template support, Python linting, and parallel execution.

## Key Features

- **Incremental builds** using SHA-256 checksums to detect changes
- **Dependency graph** with topological sort for correct build order
- **Parallel execution** - run independent products concurrently with `-j` flag
- **Template processing** via the Tera templating engine
- **Python linting** with ruff (configurable)
- **Python configuration** - load config from `.py` files using `load_python()` function
- **CLI** built with clap with shell completion support

## Main Commands

- `rsb build` - Incremental build (only rebuilds changed files)
- `rsb build --force` - Force full rebuild
- `rsb build -j4` - Build with 4 parallel jobs
- `rsb clean` - Remove build artifacts and cache
- `rsb graph` - Print dependency graph (formats: dot, mermaid, json, text)
- `rsb graph --view` - Open graph in browser (mermaid) or as SVG (dot)
- `rsb complete [shell]` - Generate shell completions

## Configuration (rsb.toml)

```toml
[build]
parallel = 1  # Number of parallel jobs (1 = sequential, 0 = auto-detect CPU cores)

[processors]
enabled = ["template", "lint", "sleep"]

[cache]
restore_method = "hardlink"  # or "copy" (hardlink is faster, copy works across filesystems)

[template]
strict = true           # Fail on undefined variables (default: true)
extensions = [".tera"]  # File extensions to process
trim_blocks = false     # Remove newline after block tags

[lint]
linter = "ruff"
args = []

[completions]
shells = ["bash"]
```

## Project Structure

```
project/
├── rsb.toml          # Configuration file
├── config/           # Python config files
├── templates/        # .tera template files
├── sleep/            # .sleep files (for parallel testing)
├── out/              # Generated stub files (lint, sleep)
└── .rsb_cache.json   # Auto-generated checksum cache
```

## Architecture

- **Processors** implement `ProductDiscovery` trait (template, lint, sleep)
- **Products** have inputs (source files) and outputs (generated files)
- **BuildGraph** manages dependencies between products
- **Executor** runs products in dependency order, with optional parallelism

## How Templates Work

- Files matching configured extensions in `templates/` generate output files in project root
- Default: `templates/{X}.tera` → `{X}`
- Templates use `load_python(path="config/settings.py")` to load Python variables

## Philosophy

Convention over configuration - simple naming conventions, explicit config loading, incremental builds by default.
