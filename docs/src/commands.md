# Command Reference

## `rsb build`

Incremental build — only rebuilds products whose inputs have changed.

```bash
rsb build                         # Incremental build
rsb build --force                 # Force full rebuild
rsb build -j4                     # Build with 4 parallel jobs
rsb build -v 2                    # Show source paths in output
rsb build --dry-run               # Show what would be built without executing
rsb build --keep-going            # Continue after errors
rsb build --timings               # Show per-product and total timing info
```

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

Show source file dependencies (e.g., header files for C/C++ sources).

```bash
rsb deps all                    # Show dependencies for all source files
rsb deps for src/main.c         # Show dependencies for a specific file
rsb deps for src/a.c src/b.c    # Show dependencies for multiple files
```

Example output:

```
src/main.c: (no dependencies)
src/test.c:
  src/utils.h
  src/config.h
```

This command is useful for:
- Debugging why a file is being rebuilt
- Understanding the include structure of your C/C++ project
- Verifying that the dependency scanner is finding the right headers

## `rsb config`

Show or inspect the configuration.

```bash
rsb config show           # Show the active configuration (defaults merged with rsb.toml)
rsb config show-default   # Show the default configuration (without rsb.toml overrides)
```

## `rsb processor`

```bash
rsb processor list          # List available processors and their status
rsb processor all           # Show all processors with descriptions
rsb processor auto          # Auto-detect which processors are relevant for this project
rsb processor files         # Show source and target files for each enabled processor
rsb processor files ruff    # Show files for a specific processor
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
