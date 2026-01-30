# RSB - Rust Build Tool Summary

A fast, incremental build tool written in Rust with template support, Python linting, and parallel execution.

## Key Features

- **Incremental builds** using SHA-256 checksums to detect changes
- **Dependency graph** with topological sort for correct build order
- **Parallel execution** - run independent products concurrently with `-j` flag
- **Template processing** via the Tera templating engine
- **Python linting** with ruff and pylint processors
- **Python configuration** - load config from `.py` files using `load_python()` function
- **CLI** built with clap with shell completion support

## Main Commands

- `rsb build` - Incremental build (only rebuilds changed files)
- `rsb build --force` - Force full rebuild
- `rsb build -j4` - Build with 4 parallel jobs
- `rsb build --processor-verbose 2` - Show source paths in build output
- `rsb build --dry-run` - Show what would be built without executing
- `rsb build --keep-going` - Continue after errors
- `rsb build --timings` - Show per-product and total timing info
- `rsb clean` - Remove build artifacts (preserves cache)
- `rsb distclean` - Remove all build directories (.rsb/ and out/) in one shot
- `rsb status` - Show product status (up-to-date, stale, or restorable)
- `rsb init` - Initialize a new rsb project in the current directory
- `rsb watch` - Watch source files and auto-rebuild on changes
- `rsb graph` - Print dependency graph (formats: dot, mermaid, json, text)
- `rsb graph --view` - Open graph in browser (mermaid) or as SVG (dot)
- `rsb cache clear` - Clear the entire cache
- `rsb cache size` - Show cache size
- `rsb cache trim` - Remove unreferenced objects from cache
- `rsb cache list` - List all cache entries and their status
- `rsb config show` - Show the active configuration (merged defaults + rsb.toml)
- `rsb processor list` - List available processors and their status
- `rsb processor auto` - Auto-detect which processors are relevant for this project
- `rsb complete [shell]` - Generate shell completions
- `rsb version` - Print version information

## Configuration (rsb.toml)

```toml
[build]
parallel = 1  # Number of parallel jobs (1 = sequential, 0 = auto-detect CPU cores)

[processor]
enabled = ["template", "ruff", "pylint", "sleep", "cc_single_file", "cpplint", "spellcheck"]

[cache]
restore_method = "hardlink"  # or "copy" (hardlink is faster, copy works across filesystems)

[graph]
viewer = "google-chrome"  # Command to open graph files (default: platform-specific)

[completions]
shells = ["bash"]
```

Per-processor configuration is documented in `docs/src/processors/`.

## Project Structure

```
project/
├── rsb.toml              # Configuration file
├── .spellcheck-words     # Custom words for spellcheck (one per line)
├── config/               # Python config files
├── templates/            # .tera template files
├── src/                  # C/C++ source files
├── sleep/                # .sleep files (for parallel testing)
├── out/
│   ├── cc_single_file/   # Compiled executables
│   ├── ruff/             # Ruff lint stub files
│   ├── pylint/           # Pylint lint stub files
│   ├── cpplint/          # C/C++ lint stub files
│   ├── spellcheck/       # Spellcheck stub files
│   └── sleep/            # Sleep stub files
├── docs/
│   └── processors/       # Per-processor documentation
└── .rsb/                 # Cache (index.json, objects/, deps/)
```

## Architecture

- **Processors** implement `ProductDiscovery` trait (template, ruff, pylint, sleep, cc_single_file, cpplint, spellcheck)
- **Products** have inputs (source files) and outputs (generated files)
- **BuildGraph** manages dependencies between products
- **Executor** runs products in dependency order, with optional parallelism
- **Build order** is deterministic — file discovery, processor iteration, and topological sort are all sorted
- **Config-aware caching** — processor config (compiler flags, linter args, etc.) is hashed into cache keys so config changes trigger rebuilds

## Philosophy

Convention over configuration - simple naming conventions, explicit config loading, incremental builds by default.
