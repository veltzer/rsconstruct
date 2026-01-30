# Command Reference

## `rsb build`

Incremental build — only rebuilds products whose inputs have changed.

```bash
rsb build                         # Incremental build
rsb build --force                 # Force full rebuild
rsb build -j4                     # Build with 4 parallel jobs
rsb build --processor-verbose 2   # Show source paths in output
rsb build --dry-run               # Show what would be built without executing
rsb build --keep-going            # Continue after errors
rsb build --timings               # Show per-product and total timing info
```

## `rsb clean`

Remove build artifacts in `out/` while preserving the cache in `.rsb/`.

```bash
rsb clean
```

## `rsb distclean`

Remove all build and cache directories (`.rsb/` and `out/`) in one shot.

```bash
rsb distclean
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

## `rsb config`

Show or inspect the active configuration (merged defaults + rsb.toml).

```bash
rsb config show    # Show the active configuration as TOML
```

## `rsb processor`

```bash
rsb processor list   # List available processors and their status
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
