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
rsbuild --phases build                    # Show phase messages during build
rsbuild --show-child-processes build      # Show each command being executed
rsbuild --show-output build               # Show compiler/linter output even on success
rsbuild --phases --show-child-processes build # Show both phases and commands
rsbuild -O path build                     # Show output file paths in build messages
rsbuild -I all build                      # Show all input files (including headers)
```

## `rsbuild build`

Incremental build — only rebuilds products whose inputs have changed.

```bash
rsbuild build                              # Incremental build
rsbuild build --force                      # Force full rebuild
rsbuild build -j4                          # Build with 4 parallel jobs
rsbuild build --dry-run                    # Show what would be built without executing
rsbuild build --keep-going                 # Continue after errors
rsbuild build --timings                    # Show per-product and total timing info
rsbuild build --stop-after discover        # Stop after product discovery
rsbuild build --stop-after add-dependencies # Stop after dependency scanning
rsbuild build --stop-after resolve         # Stop after graph resolution
rsbuild build --stop-after classify        # Stop after classifying products
rsbuild build --show-output                # Show compiler/linter output even on success
rsbuild build --auto-add-words             # Add misspelled words to .spellcheck-words instead of failing
rsbuild build --auto-add-words -p spellcheck # Run only spellcheck and auto-add words
rsbuild build -p ruff,pylint               # Run only specific processors
rsbuild build --explain                    # Show why each product is skipped/restored/rebuilt
rsbuild build --retry 3                    # Retry failed products up to 3 times
rsbuild build --no-mtime                   # Disable mtime pre-check, always compute checksums
rsbuild build --no-summary                 # Suppress the build summary
rsbuild build --batch-size 10              # Limit batch size for batch-capable processors
rsbuild build --verify-tool-versions       # Verify tool versions against .tools.versions
```

By default, tool output (compiler messages, linter output) is only shown when a command fails. Use `--show-output` to see all output.

### Processor Shortcuts (`@` aliases)

The `-p` flag supports `@`-prefixed shortcuts that expand to groups of processors:

**By type:**
- `@checkers` — all checker processors (ruff, pylint, shellcheck, etc.)
- `@generators` — all generator processors (tera, cc_single_file, etc.)
- `@mass_generators` — all mass generator processors (pip, npm, cargo, etc.)

**By tool:**
- `@python3` — all processors that require `python3`
- `@node` — all processors that require `node`
- Any tool name works (matched against each processor's `required_tools()`)

**By processor name:**
- `@ruff` — equivalent to `ruff` (strips the `@` prefix)

Examples:

```bash
rsbuild build -p @checkers              # Run only checker processors
rsbuild build -p @generators            # Run only generator processors
rsbuild build -p @python3               # Run all Python-based processors
rsbuild build -p @checkers,tera         # Mix shortcuts with processor names
```

The `--stop-after` flag allows stopping the build at a specific phase:
- `discover` — stop after discovering products (before dependency scanning)
- `add-dependencies` — stop after adding dependencies (before resolving graph)
- `resolve` — stop after resolving the dependency graph (before execution)
- `classify` — stop after classifying products (show skip/restore/build counts)
- `build` — run the full build (default)

## `rsbuild clean`

Clean build artifacts. When run without a subcommand, removes build output files (same as `rsbuild clean outputs`).

```bash
rsbuild clean                # Remove build output files (preserves cache) [default]
rsbuild clean outputs        # Remove build output files (preserves cache)
rsbuild clean all            # Remove out/ and .rsbuild/ directories
rsbuild clean git            # Hard clean using git clean -qffxd (requires git repository)
```

## `rsbuild status`

Show product status — whether each product is up-to-date, stale, or restorable from cache.

```bash
rsbuild status
```

## `rsbuild init`

Initialize a new rsbuild project in the current directory.

```bash
rsbuild init
```

## `rsbuild watch`

Watch source files and auto-rebuild on changes.

```bash
rsbuild watch                              # Watch and rebuild on changes
rsbuild watch --auto-add-words             # Watch with spellcheck auto-add words
rsbuild watch -j4                          # Watch with 4 parallel jobs
rsbuild watch -p ruff                      # Watch and only run the ruff processor
```

The watch command accepts the same build flags as `rsbuild build` (e.g., `--jobs`, `--keep-going`, `--timings`, `--processors`, `--batch-size`, `--explain`, `--retry`, `--no-mtime`, `--no-summary`).

## `rsbuild graph`

Display the build dependency graph.

```bash
rsbuild graph show                    # Default SVG format
rsbuild graph show --format dot       # Graphviz DOT format
rsbuild graph show --format mermaid   # Mermaid format
rsbuild graph show --format json      # JSON format
rsbuild graph show --format text      # Plain text hierarchical view
rsbuild graph show --format svg       # SVG format (requires Graphviz dot)
rsbuild graph view                    # Open as SVG (default viewer)
rsbuild graph view --viewer mermaid   # Open as HTML with Mermaid in browser
rsbuild graph view --viewer svg       # Generate and open SVG using Graphviz dot
rsbuild graph stats                   # Show graph statistics (products, processors, dependencies)
```

## `rsbuild cache`

Manage the build cache.

```bash
rsbuild cache clear         # Clear the entire cache
rsbuild cache size          # Show cache size
rsbuild cache trim          # Remove unreferenced objects
rsbuild cache list          # List all cache entries and their status
rsbuild cache stale         # Show which cache entries are stale vs current
rsbuild cache stats         # Show per-processor cache statistics
rsbuild cache remove-stale  # Remove stale index entries not matching any current product
```

## `rsbuild deps`

Show or manage source file dependencies from the dependency cache. The cache is populated during builds when dependency analyzers scan source files (e.g., C/C++ headers, Python imports).

```bash
rsbuild deps list                        # List all available dependency analyzers
rsbuild deps show all                    # Show all cached dependencies
rsbuild deps show files src/main.c       # Show dependencies for a specific file
rsbuild deps show files src/a.c src/b.c  # Show dependencies for multiple files
rsbuild deps show analyzers cpp          # Show dependencies from the C/C++ analyzer
rsbuild deps show analyzers cpp python   # Show dependencies from multiple analyzers
rsbuild deps stats                       # Show statistics by analyzer
rsbuild deps clean                       # Clear the entire dependency cache
rsbuild deps clean --analyzer cpp        # Clear only C/C++ dependencies
rsbuild deps clean --analyzer python     # Clear only Python dependencies
```

Example output for `rsbuild deps show all`:

```
src/main.c: [cpp] (no dependencies)
src/test.c: [cpp]
  src/utils.h
  src/config.h
config/settings.py: [python]
  config/base.py
```

Example output for `rsbuild deps stats`:

```
cpp: 15 files, 42 dependencies
python: 8 files, 12 dependencies

Total: 23 files, 54 dependencies
```

Note: This command reads directly from the dependency cache (`.rsbuild/deps.redb`). If the cache is empty, run a build first to populate it.

This command is useful for:
- Debugging why a file is being rebuilt
- Understanding the include/import structure of your project
- Verifying that dependency analyzers are finding the right files
- Viewing statistics about cached dependencies by analyzer
- Clearing dependencies for a specific analyzer without affecting others

## `rsbuild config`

Show or inspect the configuration.

```bash
rsbuild config show           # Show the active configuration (defaults merged with rsbuild.toml)
rsbuild config show-default   # Show the default configuration (without rsbuild.toml overrides)
rsbuild config validate       # Validate the configuration for errors and warnings
```

## `rsbuild processors`

```bash
rsbuild processors list              # List processors with enabled/detected status and descriptions
rsbuild processors list -a           # Include hidden processors
rsbuild processors files             # Show source and target files for each enabled processor
rsbuild processors files ruff        # Show files for a specific processor
rsbuild processors files -a          # Include disabled and hidden processors
rsbuild processors config ruff       # Show resolved configuration for a processor
rsbuild processors defconfig ruff    # Show default configuration for a processor
```

## `rsbuild tools`

List or check external tools required by enabled processors.

```bash
rsbuild tools list              # List required tools and which processor needs them
rsbuild tools list -a           # Include tools from disabled processors
rsbuild tools check             # Verify tool versions against .tools.versions lock file
rsbuild tools lock              # Lock tool versions to .tools.versions
rsbuild tools install           # Install all missing external tools
rsbuild tools install ruff      # Install a specific tool by name
rsbuild tools install -y        # Skip confirmation prompt
rsbuild tools stats             # Show tool availability and language runtime breakdown
rsbuild tools stats --json      # Show tool stats in JSON format
rsbuild tools graph             # Show tool-to-processor dependency graph (DOT format)
rsbuild tools graph --format mermaid  # Mermaid format
rsbuild tools graph --view      # Open tool graph in browser
```

## `rsbuild tags`

Search and query frontmatter tags from markdown files.

```bash
rsbuild tags list                        # List all unique tags
rsbuild tags count                       # Show each tag with file count, sorted by frequency
rsbuild tags tree                        # Show tags grouped by prefix/category
rsbuild tags stats                       # Show statistics about the tags database
rsbuild tags files docker                # List files matching a tag (AND semantics)
rsbuild tags files docker --or k8s       # List files matching any tag (OR semantics)
rsbuild tags files level=advanced        # Match key=value tags
rsbuild tags grep deploy                 # Search for tags containing a substring
rsbuild tags grep deploy -i              # Case-insensitive tag search
rsbuild tags for-file src/main.md        # List all tags for a specific file
rsbuild tags frontmatter src/main.md     # Show raw frontmatter for a file
rsbuild tags validate                    # Validate tags against .tags file
rsbuild tags unused                      # List tags in .tags not used by any file
rsbuild tags unused --strict             # Exit with error if unused tags found (CI)
rsbuild tags init                        # Generate .tags file from current tag union
rsbuild tags add docker                  # Add a tag to the .tags file
rsbuild tags remove docker               # Remove a tag from the .tags file
rsbuild tags sync                        # Add missing tags to .tags
rsbuild tags sync --prune                # Sync and remove unused tags from .tags
```

## `rsbuild complete`

Generate shell completions.

```bash
rsbuild complete bash    # Generate bash completions
rsbuild complete zsh     # Generate zsh completions
rsbuild complete fish    # Generate fish completions
```

## `rsbuild version`

Print version information.

```bash
rsbuild version
```
