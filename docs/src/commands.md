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
rsconstruct --phases build                    # Show phase messages during build
rsconstruct --show-child-processes build      # Show each command being executed
rsconstruct --show-output build               # Show compiler/linter output even on success
rsconstruct --phases --show-child-processes build # Show both phases and commands
rsconstruct -O path build                     # Show output file paths in build messages
rsconstruct -I all build                      # Show all input files (including headers)
```

## `rsconstruct build`

Incremental build — only rebuilds products whose inputs have changed.

```bash
rsconstruct build                              # Incremental build
rsconstruct build --force                      # Force full rebuild
rsconstruct build -j4                          # Build with 4 parallel jobs
rsconstruct build --dry-run                    # Show what would be built without executing
rsconstruct build --keep-going                 # Continue after errors
rsconstruct build --timings                    # Show per-product and total timing info
rsconstruct build --stop-after discover        # Stop after product discovery
rsconstruct build --stop-after add-dependencies # Stop after dependency scanning
rsconstruct build --stop-after resolve         # Stop after graph resolution
rsconstruct build --stop-after classify        # Stop after classifying products
rsconstruct build --show-output                # Show compiler/linter output even on success
rsconstruct build --auto-add-words             # Add misspelled words to .spellcheck-words instead of failing
rsconstruct build --auto-add-words -p spellcheck # Run only spellcheck and auto-add words
rsconstruct build -p ruff,pylint               # Run only specific processors
rsconstruct build --explain                    # Show why each product is skipped/restored/rebuilt
rsconstruct build --retry 3                    # Retry failed products up to 3 times
rsconstruct build --no-mtime                   # Disable mtime pre-check, always compute checksums
rsconstruct build --no-summary                 # Suppress the build summary
rsconstruct build --batch-size 10              # Limit batch size for batch-capable processors
rsconstruct build --verify-tool-versions       # Verify tool versions against .tools.versions
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
rsconstruct build -p @checkers              # Run only checker processors
rsconstruct build -p @generators            # Run only generator processors
rsconstruct build -p @python3               # Run all Python-based processors
rsconstruct build -p @checkers,tera         # Mix shortcuts with processor names
```

The `--stop-after` flag allows stopping the build at a specific phase:
- `discover` — stop after discovering products (before dependency scanning)
- `add-dependencies` — stop after adding dependencies (before resolving graph)
- `resolve` — stop after resolving the dependency graph (before execution)
- `classify` — stop after classifying products (show skip/restore/build counts)
- `build` — run the full build (default)

## `rsconstruct clean`

Clean build artifacts. When run without a subcommand, removes build output files (same as `rsconstruct clean outputs`).

```bash
rsconstruct clean                # Remove build output files (preserves cache) [default]
rsconstruct clean outputs        # Remove build output files (preserves cache)
rsconstruct clean all            # Remove out/ and .rsconstruct/ directories
rsconstruct clean git            # Hard clean using git clean -qffxd (requires git repository)
```

## `rsconstruct status`

Show product status — whether each product is up-to-date, stale, or restorable from cache.

```bash
rsconstruct status
```

## `rsconstruct smart auto`

Auto-detect relevant processors and add them to `rsconstruct.toml`. Scans the project for files matching each processor's conventions and checks that the required tools are installed. Only adds new sections — existing processor sections are preserved.

```bash
rsconstruct smart auto
```

Example output:

```
Added 3 processor(s): pylint, ruff, shellcheck
```

## `rsconstruct init`

Initialize a new rsconstruct project in the current directory.

```bash
rsconstruct init
```

## `rsconstruct watch`

Watch source files and auto-rebuild on changes.

```bash
rsconstruct watch                              # Watch and rebuild on changes
rsconstruct watch --auto-add-words             # Watch with spellcheck auto-add words
rsconstruct watch -j4                          # Watch with 4 parallel jobs
rsconstruct watch -p ruff                      # Watch and only run the ruff processor
```

The watch command accepts the same build flags as `rsconstruct build` (e.g., `--jobs`, `--keep-going`, `--timings`, `--processors`, `--batch-size`, `--explain`, `--retry`, `--no-mtime`, `--no-summary`).

## `rsconstruct graph`

Display the build dependency graph.

```bash
rsconstruct graph show                    # Default SVG format
rsconstruct graph show --format dot       # Graphviz DOT format
rsconstruct graph show --format mermaid   # Mermaid format
rsconstruct graph show --format json      # JSON format
rsconstruct graph show --format text      # Plain text hierarchical view
rsconstruct graph show --format svg       # SVG format (requires Graphviz dot)
rsconstruct graph view                    # Open as SVG (default viewer)
rsconstruct graph view --viewer mermaid   # Open as HTML with Mermaid in browser
rsconstruct graph view --viewer svg       # Generate and open SVG using Graphviz dot
rsconstruct graph stats                   # Show graph statistics (products, processors, dependencies)
```

## `rsconstruct cache`

Manage the build cache.

```bash
rsconstruct cache clear         # Clear the entire cache
rsconstruct cache size          # Show cache size
rsconstruct cache trim          # Remove unreferenced objects
rsconstruct cache list          # List all cache entries and their status
rsconstruct cache stale         # Show which cache entries are stale vs current
rsconstruct cache stats         # Show per-processor cache statistics
rsconstruct cache remove-stale  # Remove stale index entries not matching any current product
```

## `rsconstruct deps`

Show or manage source file dependencies from the dependency cache. The cache is populated during builds when dependency analyzers scan source files (e.g., C/C++ headers, Python imports).

```bash
rsconstruct deps list                        # List all available dependency analyzers
rsconstruct deps show all                    # Show all cached dependencies
rsconstruct deps show files src/main.c       # Show dependencies for a specific file
rsconstruct deps show files src/a.c src/b.c  # Show dependencies for multiple files
rsconstruct deps show analyzers cpp          # Show dependencies from the C/C++ analyzer
rsconstruct deps show analyzers cpp python   # Show dependencies from multiple analyzers
rsconstruct deps stats                       # Show statistics by analyzer
rsconstruct deps clean                       # Clear the entire dependency cache
rsconstruct deps clean --analyzer cpp        # Clear only C/C++ dependencies
rsconstruct deps clean --analyzer python     # Clear only Python dependencies
```

Example output for `rsconstruct deps show all`:

```
src/main.c: [cpp] (no dependencies)
src/test.c: [cpp]
  src/utils.h
  src/config.h
config/settings.py: [python]
  config/base.py
```

Example output for `rsconstruct deps stats`:

```
cpp: 15 files, 42 dependencies
python: 8 files, 12 dependencies

Total: 23 files, 54 dependencies
```

Note: This command reads directly from the dependency cache (`.rsconstruct/deps.redb`). If the cache is empty, run a build first to populate it.

This command is useful for:
- Debugging why a file is being rebuilt
- Understanding the include/import structure of your project
- Verifying that dependency analyzers are finding the right files
- Viewing statistics about cached dependencies by analyzer
- Clearing dependencies for a specific analyzer without affecting others

## `rsconstruct config`

Show or inspect the configuration.

```bash
rsconstruct config show           # Show the active configuration (defaults merged with rsconstruct.toml)
rsconstruct config show-default   # Show the default configuration (without rsconstruct.toml overrides)
rsconstruct config validate       # Validate the configuration for errors and warnings
```

## `rsconstruct smart`

Smart config manipulation commands for managing processor sections in `rsconstruct.toml`.

```bash
rsconstruct smart enable pylint          # Add [processor.pylint] section
rsconstruct smart disable pylint         # Remove [processor.pylint] section
rsconstruct smart enable-all             # Add sections for all builtin processors
rsconstruct smart disable-all            # Remove all processor sections
rsconstruct smart enable-detected        # Add sections for auto-detected processors
rsconstruct smart enable-if-available    # Add sections for detected processors with tools installed
rsconstruct smart minimal                # Remove all, then add only detected processors
rsconstruct smart only ruff pylint       # Remove all, then add only listed processors
rsconstruct smart reset                  # Remove all processor sections
```

## `rsconstruct processors`

```bash
rsconstruct processors list              # List declared processors and descriptions
rsconstruct processors list -a           # Include hidden processors
rsconstruct processors files             # Show source and target files for each declared processor
rsconstruct processors files ruff        # Show files for a specific processor
rsconstruct processors files -a          # Include disabled and hidden processors
rsconstruct processors config ruff       # Show resolved configuration for a processor
rsconstruct processors defconfig ruff    # Show default configuration for a processor
```

## `rsconstruct tools`

List or check external tools required by declared processors.

```bash
rsconstruct tools list              # List required tools and which processor needs them
rsconstruct tools list -a           # Include tools from disabled processors
rsconstruct tools check             # Verify tool versions against .tools.versions lock file
rsconstruct tools lock              # Lock tool versions to .tools.versions
rsconstruct tools install           # Install all missing external tools
rsconstruct tools install ruff      # Install a specific tool by name
rsconstruct tools install -y        # Skip confirmation prompt
rsconstruct tools stats             # Show tool availability and language runtime breakdown
rsconstruct tools stats --json      # Show tool stats in JSON format
rsconstruct tools graph             # Show tool-to-processor dependency graph (DOT format)
rsconstruct tools graph --format mermaid  # Mermaid format
rsconstruct tools graph --view      # Open tool graph in browser
```

## `rsconstruct tags`

Search and query frontmatter tags from markdown files.

```bash
rsconstruct tags list                        # List all unique tags
rsconstruct tags count                       # Show each tag with file count, sorted by frequency
rsconstruct tags tree                        # Show tags grouped by prefix/category
rsconstruct tags stats                       # Show statistics about the tags database
rsconstruct tags files docker                # List files matching a tag (AND semantics)
rsconstruct tags files docker --or k8s       # List files matching any tag (OR semantics)
rsconstruct tags files level:advanced        # Match key:value tags
rsconstruct tags grep deploy                 # Search for tags containing a substring
rsconstruct tags grep deploy -i              # Case-insensitive tag search
rsconstruct tags for-file src/main.md        # List all tags for a specific file
rsconstruct tags frontmatter src/main.md     # Show raw frontmatter for a file
rsconstruct tags validate                    # Validate tags against tags_dir allowlist
rsconstruct tags unused                      # List tags in tags_dir not used by any file
rsconstruct tags unused --strict             # Exit with error if unused tags found (CI)
```

## `rsconstruct complete`

Generate shell completions.

```bash
rsconstruct complete bash    # Generate bash completions
rsconstruct complete zsh     # Generate zsh completions
rsconstruct complete fish    # Generate fish completions
```

## `rsconstruct version`

Print version information.

```bash
rsconstruct version
```
