# Command Reference

## Global Flags

These flags can be used with any command:

| Flag | Description |
|------|-------------|
| `--verbose`, `-v` | Show skip/restore/cache messages during build |
| `--file-names <N>` | File name detail level (0=basename, 1=path, 2=+source, 3=+all inputs) |
| `--process` | Print each external command before execution |
| `--show-output` | Show tool output even on success (default: only show on failure) |
| `--json` | Output in JSON Lines format (machine-readable) |
| `--phases` | Show build phase messages (discover, add_dependencies, etc.) |

Example:

```bash
rsb --phases build           # Show phase messages during build
rsb --process build          # Show each command being executed
rsb --show-output build      # Show compiler/linter output even on success
rsb --phases --process build # Show both phases and commands
```

## `rsb build`

Incremental build — only rebuilds products whose inputs have changed.

```bash
rsb build                              # Incremental build
rsb build --force                      # Force full rebuild
rsb build -j4                          # Build with 4 parallel jobs
rsb build -v 2                         # Show source paths in output
rsb build --dry-run                    # Show what would be built without executing
rsb build --keep-going                 # Continue after errors
rsb build --timings                    # Show per-product and total timing info
rsb build --stop-after discover        # Stop after product discovery
rsb build --stop-after add-dependencies # Stop after dependency scanning
rsb build --stop-after resolve         # Stop after graph resolution
rsb build --show-output                # Show compiler/linter output even on success
```

By default, tool output (compiler messages, linter output) is only shown when a command fails. Use `--show-output` to see all output.

The `--stop-after` flag allows stopping the build at a specific phase:
- `discover` — stop after discovering products (before dependency scanning)
- `add-dependencies` — stop after adding dependencies (before resolving graph)
- `resolve` — stop after resolving the dependency graph (before execution)
- `build` — run the full build (default)

## `rsb clean`

Clean build artifacts. When run without a subcommand, removes build output files (same as `rsb clean outputs`).

```bash
rsb clean                # Remove build output files (preserves cache) [default]
rsb clean outputs        # Remove build output files (preserves cache)
rsb clean all            # Remove out/ and .rsb/ directories
rsb clean git            # Hard clean using git clean -qffxd (requires git repository)
```

## `rsb status`

Show product status — whether each product is up-to-date, stale, or restorable from cache.

```bash
rsb status
```

## `rsb init`

Initialize a new rsb project in the current directory.

```bash
rsb init
```

## `rsb watch`

Watch source files and auto-rebuild on changes.

```bash
rsb watch
```

## `rsb graph`

Print the dependency graph in various formats.

```bash
rsb graph                    # Default text format
rsb graph --format dot       # Graphviz DOT format
rsb graph --format mermaid   # Mermaid format
rsb graph --format json      # JSON format
rsb graph --view             # Open in browser (mermaid) or as SVG (dot)
```

## `rsb cache`

Manage the build cache.

```bash
rsb cache clear    # Clear the entire cache
rsb cache size     # Show cache size
rsb cache trim     # Remove unreferenced objects
rsb cache list     # List all cache entries and their status
```

## `rsb deps`

Show or manage source file dependencies from the dependency cache. The cache is populated during builds when dependency analyzers scan source files (e.g., C/C++ headers, Python imports).

```bash
rsb deps list                        # List all available dependency analyzers
rsb deps show all                    # Show all cached dependencies
rsb deps show files src/main.c       # Show dependencies for a specific file
rsb deps show files src/a.c src/b.c  # Show dependencies for multiple files
rsb deps show analyzers cpp          # Show dependencies from the C/C++ analyzer
rsb deps show analyzers cpp python   # Show dependencies from multiple analyzers
rsb deps stats                       # Show statistics by analyzer
rsb deps clean                       # Clear the entire dependency cache
rsb deps clean --analyzer cpp        # Clear only C/C++ dependencies
rsb deps clean --analyzer python     # Clear only Python dependencies
```

Example output for `rsb deps show all`:

```
src/main.c: [cpp] (no dependencies)
src/test.c: [cpp]
  src/utils.h
  src/config.h
config/settings.py: [python]
  config/base.py
```

Example output for `rsb deps stats`:

```
cpp: 15 files, 42 dependencies
python: 8 files, 12 dependencies

Total: 23 files, 54 dependencies
```

Note: This command reads directly from the dependency cache (`.rsb/deps.redb`). If the cache is empty, run a build first to populate it.

This command is useful for:
- Debugging why a file is being rebuilt
- Understanding the include/import structure of your project
- Verifying that dependency analyzers are finding the right files
- Viewing statistics about cached dependencies by analyzer
- Clearing dependencies for a specific analyzer without affecting others

## `rsb config`

Show or inspect the configuration.

```bash
rsb config show           # Show the active configuration (defaults merged with rsb.toml)
rsb config show-default   # Show the default configuration (without rsb.toml overrides)
```

## `rsb processors`

```bash
rsb processors list          # List available processors and their status
rsb processors all           # Show all processors with descriptions
rsb processors auto          # Auto-detect which processors are relevant for this project
rsb processors files         # Show source and target files for each enabled processor
rsb processors files ruff    # Show files for a specific processor
```

## `rsb tools`

List or check external tools required by enabled processors.

```bash
rsb tools list     # List required tools and which processor needs them
rsb tools check    # Check if required tools are available on PATH
rsb tools list -a  # Include tools from disabled processors
rsb tools check -a # Check tools from all processors
```

## `rsb complete`

Generate shell completions.

```bash
rsb complete bash    # Generate bash completions
rsb complete zsh     # Generate zsh completions
rsb complete fish    # Generate fish completions
```

## `rsb version`

Print version information.

```bash
rsb version
```
