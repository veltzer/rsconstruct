# Command Reference

## Global Flags

These flags can be used with any command:

| Flag | Description |
|------|-------------|
| `--verbose`, `-v` | Show skip/restore/cache messages during build |
| `--output-display`, `-O` | What to show for output files (`none`, `basename`, `path`; default: `none`) |
| `--input-display`, `-I` | What to show for input files (`none`, `source`, `all`; default: `source`) |
| `--path-format`, `-P` | Path format for displayed files (`basename`, `path`; default: `path`) |
| `--show-child-processes` | Print each child process command before execution |
| `--show-output` | Show tool output even on success (default: only show on failure) |
| `--json` | Output in JSON Lines format (machine-readable) |
| `--quiet`, `-q` | Suppress all output except errors (useful for CI) |
| `--phases` | Show build phase messages (discover, add_dependencies, etc.) |

Example:

```bash
rsb --phases build                    # Show phase messages during build
rsb --show-child-processes build      # Show each command being executed
rsb --show-output build               # Show compiler/linter output even on success
rsb --phases --show-child-processes build # Show both phases and commands
rsb -O path build                     # Show output file paths in build messages
rsb -I all build                      # Show all input files (including headers)
```

## `rsb build`

Incremental build — only rebuilds products whose inputs have changed.

```bash
rsb build                              # Incremental build
rsb build --force                      # Force full rebuild
rsb build -j4                          # Build with 4 parallel jobs
rsb build --dry-run                    # Show what would be built without executing
rsb build --keep-going                 # Continue after errors
rsb build --timings                    # Show per-product and total timing info
rsb build --stop-after discover        # Stop after product discovery
rsb build --stop-after add-dependencies # Stop after dependency scanning
rsb build --stop-after resolve         # Stop after graph resolution
rsb build --stop-after classify        # Stop after classifying products
rsb build --show-output                # Show compiler/linter output even on success
rsb build --auto-add-words             # Add misspelled words to .spellcheck-words instead of failing
rsb build --auto-add-words -p spellcheck # Run only spellcheck and auto-add words
rsb build -p ruff,pylint               # Run only specific processors
rsb build --explain                    # Show why each product is skipped/restored/rebuilt
rsb build --retry 3                    # Retry failed products up to 3 times
rsb build --no-mtime                   # Disable mtime pre-check, always compute checksums
rsb build --no-summary                 # Suppress the build summary
rsb build --batch-size 10              # Limit batch size for batch-capable processors
rsb build --verify-tool-versions       # Verify tool versions against .tools.versions
```

By default, tool output (compiler messages, linter output) is only shown when a command fails. Use `--show-output` to see all output.

The `--stop-after` flag allows stopping the build at a specific phase:
- `discover` — stop after discovering products (before dependency scanning)
- `add-dependencies` — stop after adding dependencies (before resolving graph)
- `resolve` — stop after resolving the dependency graph (before execution)
- `classify` — stop after classifying products (show skip/restore/build counts)
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
rsb watch                              # Watch and rebuild on changes
rsb watch --auto-add-words             # Watch with spellcheck auto-add words
rsb watch -j4                          # Watch with 4 parallel jobs
rsb watch -p ruff                      # Watch and only run the ruff processor
```

The watch command accepts the same build flags as `rsb build` (e.g., `--jobs`, `--keep-going`, `--timings`, `--processors`, `--batch-size`, `--explain`, `--retry`, `--no-mtime`, `--no-summary`).

## `rsb graph`

Display the build dependency graph.

```bash
rsb graph show                    # Default SVG format
rsb graph show --format dot       # Graphviz DOT format
rsb graph show --format mermaid   # Mermaid format
rsb graph show --format json      # JSON format
rsb graph show --format text      # Plain text hierarchical view
rsb graph show --format svg       # SVG format (requires Graphviz dot)
rsb graph view                    # Open as SVG (default viewer)
rsb graph view --viewer mermaid   # Open as HTML with Mermaid in browser
rsb graph view --viewer svg       # Generate and open SVG using Graphviz dot
rsb graph stats                   # Show graph statistics (products, processors, dependencies)
```

## `rsb cache`

Manage the build cache.

```bash
rsb cache clear         # Clear the entire cache
rsb cache size          # Show cache size
rsb cache trim          # Remove unreferenced objects
rsb cache list          # List all cache entries and their status
rsb cache stale         # Show which cache entries are stale vs current
rsb cache stats         # Show per-processor cache statistics
rsb cache remove-stale  # Remove stale index entries not matching any current product
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
rsb config validate       # Validate the configuration for errors and warnings
```

## `rsb processors`

```bash
rsb processors list              # List processors with enabled/detected status and descriptions
rsb processors list -a           # Include hidden processors
rsb processors files             # Show source and target files for each enabled processor
rsb processors files ruff        # Show files for a specific processor
rsb processors files -a          # Include disabled and hidden processors
rsb processors config ruff       # Show resolved configuration for a processor
rsb processors defconfig ruff    # Show default configuration for a processor
```

## `rsb tools`

List or check external tools required by enabled processors.

```bash
rsb tools list              # List required tools and which processor needs them
rsb tools list -a           # Include tools from disabled processors
rsb tools check             # Verify tool versions against .tools.versions lock file
rsb tools lock              # Lock tool versions to .tools.versions
rsb tools install           # Install all missing external tools
rsb tools install ruff      # Install a specific tool by name
rsb tools install -y        # Skip confirmation prompt
rsb tools stats             # Show tool availability and language runtime breakdown
rsb tools stats --json      # Show tool stats in JSON format
rsb tools graph             # Show tool-to-processor dependency graph (DOT format)
rsb tools graph --format mermaid  # Mermaid format
rsb tools graph --view      # Open tool graph in browser
```

## `rsb tags`

Search and query frontmatter tags from markdown files.

```bash
rsb tags list                        # List all unique tags
rsb tags count                       # Show each tag with file count, sorted by frequency
rsb tags tree                        # Show tags grouped by prefix/category
rsb tags stats                       # Show statistics about the tags database
rsb tags files docker                # List files matching a tag (AND semantics)
rsb tags files docker --or k8s       # List files matching any tag (OR semantics)
rsb tags files level=advanced        # Match key=value tags
rsb tags grep deploy                 # Search for tags containing a substring
rsb tags grep deploy -i              # Case-insensitive tag search
rsb tags for-file src/main.md        # List all tags for a specific file
rsb tags frontmatter src/main.md     # Show raw frontmatter for a file
rsb tags validate                    # Validate tags against .tags file
rsb tags unused                      # List tags in .tags not used by any file
rsb tags unused --strict             # Exit with error if unused tags found (CI)
rsb tags init                        # Generate .tags file from current tag union
rsb tags add docker                  # Add a tag to the .tags file
rsb tags remove docker               # Remove a tag from the .tags file
rsb tags sync                        # Add missing tags to .tags
rsb tags sync --prune                # Sync and remove unused tags from .tags
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
