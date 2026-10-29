# Command Reference

A command that takes a subcommand (`clean`, `cache`, `processor`, ...) lists
its subcommands, each with a one-line description, when run without one, and
exits with status 2. The listing leaves the flags out; `--help` shows them.

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
| `--all` | Show every column of every table, including the ones hidden by default and the ones the user config leaves out |
| `--fields` | Print each table's name and column names only, no data |
| `--phases` | Show build phase messages (discover, add_dependencies, etc.) |
| `--graph-stats` | Print graph size (product and edge counts) at each major build stage |
| `--no-mtime-cache` | Disable the persistent mtime checksum cache (useful for CI/CD where the cache won't survive the build and the write overhead isn't worth it) |
| `--color <COLOR>` | When to use ANSI color output: `auto` (tty only, the default), `always`, or `never`; the `NO_COLOR` env var also sets it to `never` |

Example:

```bash
rsconstruct --phases build                    # Show phase messages during build
rsconstruct --show-child-processes build      # Show each command being executed
rsconstruct --show-output build               # Show compiler/linter output even on success
rsconstruct --phases --show-child-processes build # Show both phases and commands
rsconstruct -O path build                     # Show output file paths in build messages
rsconstruct -I all build                      # Show all input files (including headers)
rsconstruct --all processor list              # Every column, Description included
rsconstruct --fields tool list                # The table's name and columns, no rows
```

### Table columns

Every table a command prints has a name and a fixed set of columns, and
some columns are hidden unless `--verbose` asks for them (`processor list`
hides `Description`, `hook` hides `Function` and `Location`). Which columns
show is decided in this order:

1. `--fields`: the table's name, the command that prints it, and its
   column names, with no data rows. This is how to find the names for the
   next two.
2. `--all`: every column.
3. `[tables]` in the user config, `~/.config/rsconstruct/config.toml`:
   exactly the columns listed there, in that order. See
   [`[tables]`](configuration.md#tables) for the file format and the list
   of every table.
4. Otherwise the default: every column except the hidden ones, which
   `--verbose` adds back.

`--all` goes before the command name on `tool install machine` and
`tool list-configured`, which have an `--all` of their own
(`rsconstruct --all tool list-configured`). JSON output is not affected by
any of this: `--json` always carries every field.

## `rsconstruct build`

**Requires config.** (no subcommands)

Incremental build — only rebuilds products whose inputs have changed.

```bash
rsconstruct build                              # Incremental build
rsconstruct build --force                      # Force full rebuild
rsconstruct build -j4                          # Build with 4 parallel jobs
rsconstruct build --dry-run                    # Show what would be built without executing (honors -p/-x/-t)
rsconstruct build --keep-going                 # Continue after errors
rsconstruct build --timings                    # Show per-product and total timing info
rsconstruct build --stop-after discover        # Stop after product discovery
rsconstruct build --stop-after add-dependencies # Stop after dependency scanning
rsconstruct build --stop-after resolve         # Stop after graph resolution
rsconstruct build --stop-after classify        # Stop after classifying products
rsconstruct build --show-output                # Show compiler/linter output even on success
rsconstruct build --auto-add-words             # Add misspelled words to .zspell-words instead of failing
rsconstruct build --auto-add-words -p processor.checker.zspell   # Run only zspell and auto-add words
rsconstruct build -p processor.checker.ruff,processor.checker.pylint               # Run only specific processors
rsconstruct build --explain                    # Show why each product is skipped/restored/rebuilt
rsconstruct build --retry 3                    # Retry failed products up to 3 times
rsconstruct build --no-mtime                   # Disable mtime pre-check, always compute checksums
rsconstruct build --no-summary                 # Suppress the build summary
rsconstruct build --batch-size 10              # Limit batch size for batch-capable processors
rsconstruct build --verify-tool-versions       # Verify the tools of the processors that build against rsconstruct.lock (always on in CI)
rsconstruct build -t "src/*.c"                 # Only build products matching this glob pattern
rsconstruct build -d src                       # Only build products under this directory
rsconstruct build --show-all-config-changes    # Show all config changes, not just output-affecting
rsconstruct build --trace build.json           # Write a Chrome trace JSON file for build visualization (open in chrome://tracing or Perfetto)
rsconstruct build --iset processor.checker.ruff.max_jobs=2   # Override a per-instance config field (<iname>.<field>=<value>, repeatable; value parsed as TOML)
rsconstruct build --pset processor.checker.ruff.args='["--fix"]'   # Override a field on every instance of a processor type (<pname>.<field>=<value>, repeatable; errors if no instance exists)
```

By default, tool output (compiler messages, linter output) is only shown when a command fails. Use `--show-output` to see all output.

`--verify-tool-versions` (always on when `CI=true`) checks `rsconstruct.lock`
inside the build, after discovery and the `-p`/`-x` selection: the processors
with products in the build graph must have their tools locked and installed at
the locked versions. A
disabled stanza, a processor excluded with `-x`, or one that matched no file is
not checked, exactly like the missing-tool preflight. The whole-project, exact
test is `rsconstruct tool check`; see [Tool Lock](tool-lock.md).

### Selecting processors (`-p`, `-x`)

`-p` runs the named processors **and the processors that generate their
inputs**, transitively — selecting a checker of generated files also runs
the generator, so the checker has something to check even on a clean
checkout. (`-t`/`-d` do the same for the products they select.)

`-x` excludes a processor, and with it every product that needs its
outputs: those could only run against files nobody is producing. An
excluded processor is never pulled back in, even when a `-p` selection
depends on it.

### Incremental recovery and batch behavior

Batch-capable processors run their rebuilding products in a single tool invocation, with or without `--keep-going`. A batch-capable checker waits until every one of its products is ready (its files that depend on a generated file included) and then runs once over all of them; a batch-capable generator's products run as they become ready, since one of them may feed another. If the tool fails, all products in the batch are marked failed and must be rebuilt. Use `--batch-size N` to limit batch chunks and improve recovery granularity, or `--batch-size -1` to run every product on its own; successfully completed products are cached as soon as their invocation finishes, so after a failure or an interruption the next run only rebuilds what wasn't completed.

### Processor Shortcuts (`@` aliases)

The `-p` flag supports `@`-prefixed shortcuts that expand to groups of processors:

**By type:**
- `@checker` — all checker processors (ruff, pylint, shellcheck, etc.)
- `@generator` — all generator processors (tera, cc_single_file, etc.)
- `@explicit` — all explicit processors
- `@mass_generator` — all mass generator processors

**By tool or name** — any other `@x` selects both of these:
- every processor that requires tool `x` (`@python3`, `@node`; matched
  against each processor's `required_tools()`)
- every processor whose short name is `x`, with all its instances
  (`@tera` → `processor.generator.tera` and e.g.
  `processor.generator.tera.docs`; `@generic` → the generic processor of
  every type)

An `@x` that selects nothing is reported as an unknown processor.

Examples:

```bash
rsconstruct build -p @checker              # Run only checker processors
rsconstruct build -p @generator            # Run only generator processors
rsconstruct build -p @python3               # Run all Python-based processors
rsconstruct build -p @checker,tera         # Mix shortcuts with processor names
```

The `--stop-after` flag allows stopping the build at a specific phase:
- `discover` — stop after discovering products (before dependency scanning)
- `add-dependencies` — stop after adding dependencies (before resolving graph)
- `resolve` — stop after resolving the dependency graph (before execution)
- `classify` — stop after classifying products (show skip/restore/build counts)
- `build` — run the full build (default)

## `rsconstruct clean`

Clean build artifacts. A subcommand is required; run without one, `clean` lists them.

| Subcommand | Config required? |
|------------|-----------------|
| `outputs` | Yes |
| `all` | Yes |
| `git` | Yes |
| `unknown` | Yes |

Two nouns, two depths: `clean outputs` removes output files and keeps the
cache, so the next build restores them; `clean all` removes outputs *and*
cache. Both take `-p` to limit themselves to some processors, so
`clean all -p generator.tera` removes everything of tera, files and cache
entries, and the next build runs its products again.

```bash
rsconstruct clean                # List the subcommands
rsconstruct clean outputs        # Remove build output files (preserves cache)
rsconstruct clean outputs -p generator.tera,generator.tags    # Only clean outputs of these processors
rsconstruct clean outputs --no-empty-dirs # Skip the empty-directory sweep
rsconstruct clean all            # Remove out/ and .rsconstruct/ directories
rsconstruct clean all -p generator.tera   # tera's files and cache entries; re-rendered next build
rsconstruct clean all -p @checker         # Forget every check; all lint runs again
rsconstruct clean all -p mass_generator.linux_module --dry-run   # Show what would go
rsconstruct clean git            # Hard clean using git clean -qffxd (requires git repository)
rsconstruct clean unknown        # Remove files not tracked by git and not known as build outputs
rsconstruct clean unknown --dry-run      # Show what would be removed without deleting
rsconstruct clean unknown --no-gitignore # Include gitignored files as unknown
```

### `rsconstruct clean outputs`

Removes build output files. The cache (`.rsconstruct/`) is preserved.

What gets removed depends on each product's processor type:

- **Checkers** (ruff, cppcheck, shellcheck, terms, …) — produce no outputs, so nothing to delete.
- **Generators** (tera, tags, mako, jinja2, mermaid, pdflatex, pdfunite, …) — declare concrete output files. Each declared output file is removed individually with `fs::remove_file`. **Directories are never recursed into.**
- **Explicit** — declares both `output_files` and `output_dirs` directly in `rsconstruct.toml`. Files are removed individually; declared `output_dirs` are removed recursively (because the user asked for that). This is the only path that recursively deletes a directory.
- **Mass generators** — every file the tool's plan predicted is a declared output and is removed individually. The `output_dirs` a mass generator declares only bound where predicted files may live and are never removed.

#### Empty-directory sweep

After every product's `clean()` has run, `clean outputs` walks every directory that previously contained an output file (and the parents of every removed `output_dir`), bottom-up, and removes any that are now empty. The sweep:

- Tries `fs::remove_dir` (non-recursive) — only succeeds when the directory is empty.
- Walks upward through parents until it hits a non-empty directory or the project root.
- Never removes a directory that still contains files — empty dirs only.
- Does not special-case `out/`. Any path ancestor of a cleaned output is eligible.

Pass `--no-empty-dirs` to skip the sweep entirely. Files still get cleaned; empty parent directories are left in place.

#### Filtering by processor

`-p` / `--processor` takes a comma-separated list of the same selectors as
[`build -p`](#rsconstruct-build): a processor name with or without the
`processor.` prefix (`generator.tera`, as the tables print it), an instance
name (`generator.marp.slides`), `@checker` for a type, `@python3` for every
processor that needs a tool, `@tera` for a short name with all its
instances. A selector that matches nothing is an error listing the
processors in the config. Only the selected processors' products are
cleaned.

### `rsconstruct clean all`

Without `-p`, removes the output directory (`out/` by default) and
`.rsconstruct/` outright: every output and the whole cache. The one thing
kept is `.rsconstruct/tools/`, the project's installed tools (see
[Tool Lock](tool-lock.md#where-tools-are-installed)), which are not build
state. With `-p`,
removes everything of the selected processors only: their output files, as
`clean outputs` does, and the cache entries for their products, so the next
build runs them again instead of restoring them. Use the `-p` form to make
one processor redo its work without touching the rest of the cache:

```bash
rsconstruct clean all                              # everything
rsconstruct clean all -p generator.tera            # everything of tera
rsconstruct clean all -p @checker --dry-run        # what would go
```

- `-p` takes the selectors described under
  [Filtering by processor](#filtering-by-processor).
- `--dry-run` names the directories (without `-p`) or counts the files,
  output directories and cache entries (with `-p`; `--verbose` lists the
  files) that would go, and removes nothing.
- `--no-empty-dirs` skips the empty-directory sweep of the `-p` form, as for
  `clean outputs`.

With `-p`, the cache entry removed is the one for each product's current
inputs, the key the next build would look up. Entries left over from earlier
inputs are what `cache remove-stale` removes, and the blobs they point at
are what `cache trim` removes; `clean all -p` touches neither. Products
downstream of a cleaned one are not cleaned: they run again on the next
build if the regenerated output differs, and are rightly skipped if it is
byte-identical. With a remote cache and pull enabled, the next build may
still restore from the remote.

`clean all -p` replaced the `clean products` subcommand of 0.9.133.

#### Other clean variants

- `clean all` — removes `out/` and `.rsconstruct/` outright (cache included). Use when you want a full reset.
- `clean git` — runs `git clean -qffxd`. Requires a git repository.
- `clean unknown` — finds files in the working tree that are neither git-tracked nor declared as a build output, and deletes them. Pass `--dry-run` to only list them. `--no-gitignore` includes gitignored files in the search.

## `rsconstruct status`

**Requires config.** (no subcommands)

Show product status — whether each product is up-to-date, stale, or restorable from cache.

```bash
rsconstruct status                     # Show per-processor and total summary
rsconstruct status -v                  # Show per-product status
rsconstruct status --breakdown         # Show source file counts by processor and extension
rsconstruct --json status              # Same summary as JSON
```

The per-processor table (and each entry of `--json`'s `processor` array) carries
two flags after the counts: `builtin`, whether the processor is pure Rust inside
rsconstruct, and `rust`, whether the code that does the work is written in Rust at
all — always so for a builtin processor, and for an external one only when the tool
it runs is (ruff, taplo, rumdl, clippy, cargo, mdbook, pyrefly, rustc). Processors
that run a user-supplied command (`script` and the `generic` processors of the
`generator`, `explicit` and `mass_generator` types) report `rust: false`, since the language is unknown.

## `rsconstruct init`

**No config needed.** (no subcommands)

Initialize a new rsconstruct project in the current directory.

```bash
rsconstruct init
```

## `rsconstruct watch`

**Requires config.** (no subcommands)

Watch source files and auto-rebuild on changes.

```bash
rsconstruct watch                              # Watch and rebuild on changes
rsconstruct watch --auto-add-words             # Watch with zspell auto-add words
rsconstruct watch -j4                          # Watch with 4 parallel jobs
rsconstruct watch -p processor.checker.ruff                      # Watch and only run the ruff processor
```

The watch command accepts the same build flags as `rsconstruct build` (e.g., `--jobs`, `--keep-going`, `--timings`, `--processor`, `--batch-size`, `--explain`, `--retry`, `--no-mtime`, `--no-summary`).

Each rebuild is a fresh incremental build:

- The in-session caches (per-file checksums, the combined-input memo for
  large input lists) are cleared before every rebuild. They assume file
  content is stable for one build run, and the edit that triggered the
  rebuild is exactly what they would otherwise hide — a product with many
  inputs was never rebuilt in watch mode before this.
- `rsconstruct.toml` is re-read before every rebuild, including
  `[cache] mtime_check`. The CLI `--no-mtime-cache` flag stays authoritative:
  it keeps the mtime pre-check off whatever the config says.
- Events under `.rsconstruct/` and under `[build] output_dir` are ignored,
  matched as path prefixes relative to the project root. A multi-segment
  `output_dir = "build/out"` is honored, and a source directory that merely
  shares the output directory's name (`src/out/foo.rs` with
  `output_dir = "out"`) is watched like any other.

## `rsconstruct graph`

Display the build dependency graph.

| Subcommand | Config required? |
|------------|-----------------|
| `lookup-fwd` | Yes |
| `lookup-rev` | Yes |
| `show` | Yes |
| `stats` | Yes |
| `unreferenced` | Yes |
| `view` | Yes |

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
rsconstruct graph lookup-fwd src/main.c   # For each input file, show the products that consume it (source -> outputs)
rsconstruct graph lookup-rev out/processor.generator.cc_single_file/src/main.elf   # For each output file, show the product that produced it (output -> sources)
rsconstruct graph unreferenced -e .svg,.png   # List files on disk with these extensions that no product in the graph references
```

## `rsconstruct cache`

Manage the build cache.

| Subcommand | Config required? |
|------------|-----------------|
| `clear` | No |
| `size` | Yes |
| `trim` | Yes |
| `list` | Yes |
| `stale` | Yes |
| `stats` | Yes |
| `remove-stale` | Yes |

```bash
rsconstruct cache clear         # Clear the entire cache (.rsconstruct/ except the installed tools in .rsconstruct/tools/)
rsconstruct cache size          # Show cache size
rsconstruct cache trim          # Remove unreferenced objects
rsconstruct cache list          # List all cache entries and their status
rsconstruct cache stale         # Show which cache entries are stale vs current
rsconstruct cache stats         # Show per-processor cache statistics
rsconstruct cache remove-stale  # Remove stale index entries not matching any current product
```

## `rsconstruct webcache`

Manage the web request cache. Schemas fetched by `iyamlschema` (and any future processors that fetch URLs) are cached in `.rsconstruct/webcache.redb`.

| Subcommand | Config required? |
|------------|-----------------|
| `clear` | No |
| `stats` | No |
| `list` | No |

```bash
rsconstruct webcache clear   # Clear all cached web responses
rsconstruct webcache stats   # Show cache size and entry count
rsconstruct webcache list    # List all cached URLs and their sizes
```

## `rsconstruct analyzer`

Manage dependency analyzers and inspect the dependency cache they fill. An analyzer scans source files for dependencies (C/C++ `#include`s, Python imports, Sass `@use`s, Markdown and Tera references) and records them in `.rsconstruct/deps.redb`; see [Dependency Analyzers](analyzers.md). Analyzers are referred to by pname (the type: `cpp`, `icpp`, `markdown`, `python`, `sass`, `tera`) or by iname (the instance as declared in `rsconstruct.toml`, e.g. `cpp` or `cpp.kernel`).

| Subcommand | Config required? |
|------------|-----------------|
| `add` | Yes (`--dry-run`: no) |
| `build` | Yes |
| `clean` | Yes |
| `config` | Yes |
| `defconfig` | No |
| `delete` | Yes |
| `disable` | Yes |
| `enable` | Yes |
| `list` | No |
| `show` | Yes |
| `stats` | Yes |
| `used` | Yes |

```bash
rsconstruct analyzer list                        # List all available analyzers
rsconstruct analyzer used                        # Show which analyzers are enabled and detected
rsconstruct analyzer defconfig cpp               # Show the default configuration of an analyzer type
rsconstruct analyzer config                      # Show the resolved configuration of every declared analyzer
rsconstruct analyzer config cpp                  # ...of one analyzer instance
rsconstruct analyzer add cpp                     # Append [analyzer.cpp] to rsconstruct.toml with comments
rsconstruct analyzer add cpp --dry-run           # Print the snippet instead of writing it
rsconstruct analyzer enable cpp                  # Set enabled = true on the stanza
rsconstruct analyzer disable cpp                 # Set enabled = false on the stanza
rsconstruct analyzer delete cpp                  # Remove the stanza entirely
rsconstruct analyzer build                       # Run dependency analysis without building
rsconstruct analyzer show all                    # Show all cached dependencies
rsconstruct analyzer show files src/main.c       # Show dependencies for a specific file
rsconstruct analyzer show files src/a.c src/b.c  # ...for several files
rsconstruct analyzer show files --hash-pieces src/main.c  # Also show each analyzer's hash pieces
rsconstruct analyzer show files ./src/main.c     # A leading ./ is ignored
rsconstruct analyzer show analyzers cpp          # Show dependencies found by the C/C++ analyzer
rsconstruct analyzer show analyzers cpp python   # ...by several analyzers
rsconstruct analyzer stats                       # Show statistics by analyzer
rsconstruct analyzer clean                       # Clear the entire dependency cache
rsconstruct analyzer clean --analyzer cpp        # Clear only entries from the cpp analyzer
```

Example output for `rsconstruct analyzer show all`:

```text
src/main.c: [icpp] (no dependencies)
src/test.c: [icpp]
  src/utils.h
  src/config.h
```

Example output for `rsconstruct analyzer stats`:

```text
╭──────────┬───────┬──────────────╮
│ Analyzer │ Files │ Dependencies │
├──────────┼───────┼──────────────┤
│ icpp     │ 2     │ 2            │
├──────────┼───────┼──────────────┤
│ Total    │ 2     │ 2            │
╰──────────┴───────┴──────────────╯
```

`show`, `stats` and `clean` read the dependency cache directly. If it is empty, run `rsconstruct analyzer build` (or a build) first to populate it.

These commands are useful for:
- Debugging why a file is being rebuilt
- Understanding the include/import structure of your project
- Verifying that dependency analyzers are finding the right files
- Clearing dependencies for a specific analyzer without affecting others

## `rsconstruct smart`

Smart config manipulation commands for managing processor sections in `rsconstruct.toml`.

| Subcommand | Config required? |
|------------|-----------------|
| `auto` | Yes |
| `disable` | No |
| `disable-all` | No |
| `enable` | No |
| `enable-all` | No |
| `enable-detected` | Yes |
| `enable-if-available` | Yes |
| `minimal` | Yes |
| `only` | No |
| `remove-no-file-processors` | Yes |
| `reset` | No |

Processor names are always the full name, `processor.<type>.<name>`; a short name such as `pylint` is rejected with "Unknown processor".

```bash
rsconstruct smart auto                   # Auto-detect relevant processors (files present, tool installed) and add their sections; existing sections are kept
rsconstruct smart enable processor.checker.pylint    # Add [processor.checker.pylint] section
rsconstruct smart disable processor.checker.pylint   # Remove [processor.checker.pylint] section
rsconstruct smart enable-all             # Add sections for all builtin processors
rsconstruct smart disable-all            # Remove all processor sections
rsconstruct smart enable-detected        # Add sections for auto-detected processors
rsconstruct smart enable-if-available    # Add sections for detected processors with tools installed
rsconstruct smart minimal                # Remove all, then add only detected processors
rsconstruct smart only processor.checker.ruff processor.checker.pylint   # Remove all, then add only listed processors
rsconstruct smart reset                  # Remove all processor sections
rsconstruct smart remove-no-file-processors  # Remove processors that don't match any files (skips enabled = false)
```

## `rsconstruct processor`

| Subcommand | Config required? |
|------------|-----------------|
| `add` | Yes (`--dry-run`: no) |
| `allowlist` | Yes |
| `config` | Yes |
| `defconfig` | No |
| `delete` | Yes |
| `disable` | Yes |
| `enable` | Yes |
| `files` | Yes |
| `graph` | Yes |
| `info` | No |
| `list` | No |
| `names` | Yes |
| `recommend` | No |
| `search` | No |
| `types` | No |
| `used` | Yes |

```bash
rsconstruct processor list              # List all built-in processors with type and description
rsconstruct processor list --type checker  # ...only one processor type
rsconstruct --json processor list       # Same list as JSON
rsconstruct processor info processor.checker.ruff         # Describe a processor, the tools it runs at its default config, and every config field
rsconstruct processor info              # ...every processor
rsconstruct --json processor info       # Same as JSON
rsconstruct processor types             # List the processor types
rsconstruct processor search yaml       # Search processors by name, description or keywords
rsconstruct processor recommend         # Show the recommended processor for each file extension
rsconstruct processor used              # Show the enabled processors and whether each is detected
rsconstruct processor names             # Print the iname of every enabled processor, one per line
rsconstruct processor files             # Show source and target files for each declared processor
rsconstruct processor files processor.checker.ruff        # Show files for a specific processor
rsconstruct processor config processor.checker.ruff       # Show resolved configuration for a processor
rsconstruct processor config --diff     # Show only fields that differ from defaults
rsconstruct processor defconfig processor.checker.ruff    # Show default configuration for a processor
rsconstruct processor add processor.checker.ruff          # Append [processor.checker.ruff] to rsconstruct.toml (fields pre-populated + comments)
rsconstruct processor add processor.checker.ruff --dry-run  # Preview the snippet without writing
rsconstruct processor enable processor.checker.ruff       # Set enabled = true on the stanza
rsconstruct processor disable processor.checker.ruff      # Set enabled = false on the stanza
rsconstruct processor delete processor.checker.ruff       # Remove the stanza entirely
rsconstruct processor allowlist         # Show the enabled processors as an `enabled = [...]` line
rsconstruct processor graph             # Show inter-processor dependencies
rsconstruct processor graph --format dot    # Graphviz DOT format
rsconstruct processor graph --format mermaid # Mermaid format
rsconstruct processor files --headers   # Show files with processor headers
```

`list` prints one row per processor with its type and four flags: `Builtin`
(pure Rust inside rsconstruct, or `external` when it runs another program),
`Rust` (whether the code that does the work is written in Rust — every
builtin processor, plus external ones whose tool is Rust: ruff, taplo, rumdl,
clippy, cargo, mdbook, pyrefly, rustc), `Batch` (whether the processor can
take several input files in one tool invocation — a linter that accepts a
file list can, a converter that writes one output per input such as pandoc
or marp cannot) and `Cache` (whether a cache hit stands in for a run: it
restores a generator's outputs or proves a checker's pass, so nothing is
lost by skipping the run; `No` for pip, npm and gem, which install into an
environment and write only a stamp — the stamp is cached while the installed
packages may be gone). The JSON form carries the same as `builtin`, `rust`,
`supports_batch` and `caches_outputs` booleans. `rust` is what to read when
the question is "how much of this project's toolchain is Rust"; `builtin` is
what to read when the question is "which processors need no tool
installed"; `supports_batch` is what decides whether a stanza's `batch` field
and the build's `--batch-size` have any effect on it (a processor that
cannot batch runs one file per invocation whatever they say).

In every table, processor names drop the `processor.` prefix that all of
them carry (`checker.ruff`, not `processor.checker.ruff`): it says nothing
in a column of processors and costs a word of width on every row. JSON
output, error messages and `processor info` keep the full name, which is
what `rsconstruct.toml` and scripts use.

`used`, `names` and `allowlist` list the processors that take part in builds:
every declared stanza except those with `enabled = false`. A disabled stanza
is declared but not used, so it is left out rather than shown under an
`enabled = [...]` heading. `config` still shows every stanza, disabled ones
included — their configuration is what `enable` will bring back.

A fifth column, `Rust alternative`, names the Rust processors that can
replace a non-Rust one (`ishellcheck` for `shellcheck`, `iluacheck` and
`selene` for `luacheck`); the JSON form has them as a `rust_alternatives` array, empty
where there is none. Each Rust processor declares what it replaces in its own
file, and its alternative's docs page links it.

`recommend` maps file extensions to the processor for them. Where a Rust
processor can replace the classic recommendation, the Rust one is listed and
the classic one moves to the `Also` column (`also` in the JSON, next to
`processors`, every Rust option, and `processor`, the first of them).

`info` is the long form of one processor (or, without a name, of every
processor): its description, type, keywords, the same flags as `list`, the
`max_jobs` cap and the implementation version, then a table
of every config field it accepts — type, default, whether it is required,
whether it is part of the cache key (`Checksum`), and what it does. It is
`defconfig -v` with the processor's own description on top, and reads only
the processors' built-in metadata, so it needs no `rsconstruct.toml`.

## `rsconstruct tool`

List or check external tools required by declared processors. `list` reads only the tool registry; `check` and `lock` need a project; the rest use config if available and without one operate on all built-in processors.

| Subcommand | Config required? |
|------------|-----------------|
| `check` | Yes |
| `graph` | Uses config if available |
| `install machine` | Uses config if available |
| `install project` | Yes |
| `list` | No |
| `list-configured` | Uses config if available |
| `lock` | Yes |
| `stats` | Uses config if available |

```bash
rsconstruct tool list              # List all tools known to rsconstruct (from the tool registry)
rsconstruct tool list -M           # ...with every install method for each tool
rsconstruct tool list-configured   # List tools this project's processors require, and which needs them
rsconstruct tool list-configured -a   # Include tools from disabled processors
rsconstruct tool check             # Verify rsconstruct.lock covers the project and installed tools match it
rsconstruct tool lock              # Write rsconstruct.lock: add missing entries, drop unused ones, keep the rest
rsconstruct tool lock --upgrade    # Re-resolve every rsconstruct.lock entry to its latest release
rsconstruct tool lock --upgrade ruff cargo-deny   # Re-resolve only these entries
rsconstruct tool install           # List the two targets: machine and project
rsconstruct tool install machine   # Install what this project needs on the machine: [dependencies] system packages, machine-preferred tools
rsconstruct tool install machine shellcheck   # Install one tool through its machine install
rsconstruct tool install machine --all        # Install every tool in the registry onto the machine, ignoring the config (no config needed)
rsconstruct tool install machine --no-eatmydata   # Don't wrap apt/dnf/pacman with eatmydata
rsconstruct tool install project   # Install everything into the project from its lockfiles: uv sync (.venv), npm ci (node_modules), .rsconstruct/tools
rsconstruct tool install project rumdl        # Install one tool through its project install
rsconstruct tool install project -i           # Ask for confirmation first (default: install without asking)
rsconstruct tool stats             # Show tool availability (one processor per row, then "+N more") and language runtime breakdown
rsconstruct tool stats --json      # Show tool stats in JSON format
rsconstruct tool graph             # Show tool-to-processor dependency graph (DOT format)
rsconstruct tool graph --format mermaid  # Mermaid format
rsconstruct tool graph --view      # Open tool graph in browser
```

`install` names where the install goes, and each of its two subcommands
touches only its own target:

- **`install project`** installs into the project, every package from a
  lockfile: `uv sync` builds the repo's `.venv` from `uv.lock` (the code's
  Python dependencies and the Python tools its processors run, together),
  `npm ci` fills `node_modules` from `package-lock.json`, and the crates, npm
  packages, gems and release binaries `rsconstruct.lock` pins are linked into
  `.rsconstruct/tools` from a machine-wide store. `uv` and `node` (with its
  `npm`) are among those release binaries: every Python project locks uv
  and every Node project locks node, and each is linked in ahead of the
  `uv sync` or npm steps that run it. It never touches the machine.
- **`install machine`** installs onto the machine: the `[dependencies]
  system` packages and the tools of the enabled processors whose preferred
  target is the machine (compilers, runtimes, apt-only tools, `.deb`s). A pip install
  here must not land in the project's own `.venv`, which is uv's; it is an
  error when `pip` on PATH is the project's.

A fresh checkout is set up with `install machine` followed by `install
project`. Both work from `rsconstruct.lock` in a project, together with
`check` and `lock`: installs request each tool at its locked version, and in
CI a lock that does not match the project is an error. `install project`
brings the whole tool directory in line with the lock. Every rsconstruct
invocation puts `.rsconstruct/tools/bin`, the repo's `.venv/bin` and
`node_modules/.bin` first on `PATH`, so each project runs exactly its locked
versions with nothing activated. See [Tool
Lock](tool-lock.md#where-tools-are-installed).

A Python tool a processor runs (ruff, pytest, pyrefly) must be pinned by
`uv.lock`: the repo's `.venv` is uv's, so `pyproject.toml`'s dev dependency
group (`uv add --dev ruff`) is the one place to declare it. One that is not
is an error naming that command.

The project's tools are those of its enabled processors. A stanza with
`enabled = false` is kept precisely so its tool need not be installed, so
`list-configured`, `install`, `lock`, `check`, `stats` and `graph` leave its
tools out; `list-configured --all` is the one way to see them.

`install machine --all` walks the tool registry instead of the project
config, so it needs no `rsconstruct.toml`. It never skips: a tool with no
machine install is a hard error, not a warning. This makes it the intended
way to provision a machine for every processor at once (rsconstruct's own
CI does, for its tests), and a registry entry that loses its machine install
fails the provisioning step instead of quietly shrinking the matrix. `--all`
cannot be combined with a tool name, and `install project` has none: a
project installs only what it uses.

### Install targets

Every install goes to one of two targets: the **project** (its
`.rsconstruct/tools` directory, or the venv inside the repo) or the
**machine** (a system package manager, a user-wide prefix such as
`~/.cargo/bin` or `npm -g`, or a venv outside the repo). A system package
manager (apt, dnf, pacman, brew, snap) and a `.deb` binary can only reach the
machine; a release kept whole (protoc beside its `include/`, qpdf beside its
`lib/`) needs a prefix of its own and can only reach the project; pip, uv,
npm, cargo, gem and other release binaries can reach either, depending on
where they are pointed.

Each tool in the registry names up to three preferences:

- its **project install**: the method that installs it into the project, if
  there is one;
- its **machine install**: the method that installs it onto the machine;
- its **preferred target** overall: the project when the tool has a project
  install, otherwise the machine. A registry entry can override this
  (`prefer`); none does today.

The preferred target decides which of `install project` and `install
machine` installs a tool the project needs; naming a tool (`install project
NAME`, `install machine NAME`) picks the slot directly. For pip the project
install means the repo's `.venv` through `uv sync`, never `pip install`.
Any other methods a tool
lists are alternatives (brew for macOS, a crate where a release binary is
preferred): shown by `list -M`, never chosen by an install. `list` shows the
preferred target in its Target column; `list -M` labels each method with the
slots it fills (`project`, `machine`, or `alt`); `--json tool list` carries,
per method, the targets it can reach (`targets`) and the slots it fills
(`slots`), and per tool the `preferred` target.

### Ruby gems without a writable gem dir

Only system package managers (apt/dnf/pacman/snap) are sudo-wrapped; `pip`,
`npm`, `cargo`, and `gem` run unprivileged. With a distro ruby, that would
make `gem install` die on the root-owned system gem dir (`/var/lib/gems` on
Debian/Ubuntu), so when the default gem dir isn't writable, gem installs get
`--user-install` appended automatically. To make the resulting executables
resolvable, every rsconstruct invocation appends the user gem bin dirs that
exist (`$GEM_HOME/bin`, `~/.gem/ruby/*/bin`, `~/.local/share/gem/ruby/*/bin`)
to its own `PATH` at startup — tool probes and spawned processors see them
without any workflow- or shell-level `GEM_HOME`/`PATH` setup.

### Node.js packages from the project's `node_modules`

The same startup step prepends the project's `node_modules/.bin`, when it
exists, so the packages `install project` put there from `package.json`
(with `npm ci`, see `[dependencies]` in the configuration reference) resolve
ahead of any global copy. `install machine` therefore finds a locked tool
already installed and does not add a global one on top.

### Python packages from the repo's `.venv`

In a project directory the startup step also prepends the repo's
`.venv/bin`, where `install project` (`uv sync`) puts the code's
dependencies and the Python tools its processors run. `python3`, `ruff` and
`pytest` therefore resolve to the locked copies with no venv activated, in a
shell and on a CI runner alike.

## `rsconstruct tag`

Search and query frontmatter tags from markdown files.

| Subcommand | Config required? |
|------------|-----------------|
| `list` | Yes |
| `count` | Yes |
| `tree` | Yes |
| `stats` | Yes |
| `files` | Yes |
| `grep` | Yes |
| `for-file` | Yes |
| `frontmatter` | Yes |
| `unused` | Yes |
| `validate` | Yes |
| `matrix` | Yes |
| `coverage` | Yes |
| `orphans` | Yes |
| `check` | Yes |
| `suggest` | Yes |
| `merge` | Yes |
| `collect` | Yes |

```bash
rsconstruct tag list                        # List all unique tags
rsconstruct tag count                       # Show each tag with file count, sorted by frequency
rsconstruct tag tree                        # Show tags grouped by prefix/category
rsconstruct tag stats                       # Show statistics about the tags database
rsconstruct tag files docker                # List files matching a tag (AND semantics)
rsconstruct tag files docker --or k8s       # List files matching any tag (OR semantics)
rsconstruct tag files level:advanced        # Match key:value tags
rsconstruct tag grep deploy                 # Search for tags containing a substring
rsconstruct tag grep deploy -i              # Case-insensitive tag search
rsconstruct tag for-file src/main.md        # List all tags for a specific file
rsconstruct tag frontmatter src/main.md     # Show raw frontmatter for a file
rsconstruct tag validate                    # Validate tags against tags_dir allowlist
rsconstruct tag unused                      # List tags in tags_dir not used by any file
rsconstruct tag unused --strict             # Exit with error if unused tags found (CI)
rsconstruct tag check                       # Run all tag validations without building
rsconstruct tag suggest src/main.md         # Suggest tags for a file based on similarity
rsconstruct tag coverage                    # Show percentage of files with each tag category
rsconstruct tag matrix                      # Show coverage matrix of tag categories per file
rsconstruct tag orphans                     # Find markdown files with no tags
rsconstruct tag merge ../other/tags         # Merge tags from another project
rsconstruct tag collect                     # Add missing tags from source files to tag collection
```

## `rsconstruct complete`

Generate shell completions. No config needed when shell is specified as argument; uses config to read default shells if no argument given.

```bash
rsconstruct complete bash    # Generate bash completions
rsconstruct complete zsh     # Generate zsh completions
rsconstruct complete fish    # Generate fish completions
```

## `rsconstruct term`

Manage term checking and fixing in markdown files.

| Subcommand | Config required? |
|------------|-----------------|
| `fix` | Yes |
| `merge` | Yes |
| `stats` | Yes |

### `rsconstruct term fix`

Add backticks around terms from the terms directory that appear unquoted in markdown files.

```bash
rsconstruct term fix
rsconstruct term fix --remove-non-terms   # also remove backticks from non-terms
```

### `rsconstruct term merge`

Merge terms from another project's terms directory. Unions matching files and copies missing files in both directions.

```bash
rsconstruct term merge ../other-project/terms
```

## `rsconstruct doctor`

**Requires config.** (no subcommands)

Diagnose build environment — checks config, the tools of the enabled
processors (a stanza with `enabled = false` is not asked for its tool),
versions, and declared `[dependencies]` (Python deps declared in
`pyproject.toml` are checked too; see the `[dependencies]` section in
Configuration). Tools are probed by running them; `system` dependencies are
packages, not tools, so they are probed through the platform's package manager
(dpkg-query/rpm/pacman/brew) — a binary-less package like `aspell-en` counts
as installed even though no binary carries its name.

```bash
rsconstruct doctor
```

## `rsconstruct info`

Show project information.

| Subcommand | Config required? |
|------------|-----------------|
| `source` | Yes |

```bash
rsconstruct info source          # Show source file counts by extension
```

## `rsconstruct page`

Query GitHub Pages publishing settings from the [`[pages]`](configuration.md#pages) config section.

| Subcommand | Config required? |
|------------|-----------------|
| `dir` | Yes |

```bash
rsconstruct page dir            # Print the published directory ([pages].dir)
rsconstruct --json page dir      # {"configured": true, "dir": "out/web"}
```

`page dir` prints nothing and still exits 0 when `[pages]` is not configured — CI branches on the output being empty, not on exit codes. See [GitHub Actions](github-actions.md#github-pages-deployment) for the workflow pattern this enables.

## `rsconstruct sloc`

**No config needed.** (no subcommands)

Count source lines of code (SLOC) by language, with optional COCOMO effort/cost estimation.

```bash
rsconstruct sloc                 # Show SLOC by language
rsconstruct sloc --cocomo        # Include COCOMO effort/cost estimation
rsconstruct sloc --cocomo --salary 80000  # Custom annual salary for COCOMO
```

## `rsconstruct toml`

```bash
rsconstruct toml check   # validate rsconstruct.toml (unknown fields, types, required fields)
rsconstruct toml files   # list every config file rsconstruct may read and whether it exists
```

`toml files` needs no config. It prints the merge chain lowest precedence
first — the user config (`$XDG_CONFIG_HOME/rsconstruct/config.toml`, see
[User config](configuration.md#user-config-configrsconstructconfigtoml)),
`rsconstruct.toml` and `rsconstruct.local.toml` — followed by the other
project files rsconstruct reads (`.rsconstructignore`, `rsconstruct.lock`),
each with its resolved path and whether it is present. Use it to find out
which file a setting is coming from, or where the user config lives on a
given machine. With `--json` the same list is emitted as an array of
`{precedence, role, path, exists, note}` objects.

## `rsconstruct error`

**No config needed.** (no subcommands)

List every exit code rsconstruct can return and its name. Errors are printed as `Error [EXIT_...]: ...`, so the name in a message maps back to the code here.

```bash
rsconstruct error
```

| Code | Name |
|------|------|
| 0 | `EXIT_SUCCESS` |
| 1 | `EXIT_BUILD_ERROR` |
| 2 | `EXIT_CONFIG_ERROR` |
| 3 | `EXIT_TOOL_ERROR` |
| 4 | `EXIT_GRAPH_ERROR` |
| 5 | `EXIT_IO_ERROR` |
| 130 | `EXIT_INTERRUPTED` |

## `rsconstruct function`

**No config needed.**

Inspect the built-in functions rsconstruct exposes to Tera templates (`load_python`, `load_lua`, `load_toml`, `toml_get`, ...).

| Subcommand | Config required? |
|------------|-----------------|
| `list` | No |

```bash
rsconstruct function list   # Signature, return value, dependency tracking and an example for each function
```

Example entry:

```text
load_toml(path: string)
  Parse a TOML file and expose its top-level table to the template.
  returns:       object (key → value, nested tables preserved)
  dep tracking:  The file at `path` is added as an input (content-tracked).
  example:       {% set cfg = load_toml(path="pyproject.toml") %}{{ cfg.project.version }}
```

## `rsconstruct hook`

**No config needed.** (no subcommands)

List the post-config hooks: built-in adjustments applied to the configuration after it is loaded, such as defaults that change when running under CI (`CI=true`).

```bash
rsconstruct hook
```

```text
╭──────────────────────┬─────────────────────────────────────────────────────────────────────╮
│ Hook                 │ Description                                                         │
├──────────────────────┼─────────────────────────────────────────────────────────────────────┤
│ eatmydata_ci_default │ When CI=true, enable eatmydata wrapping for apt/dnf/pacman installs │
│ marp_ci_cap          │ When CI=true and marp.max_jobs is unset, cap it at 2                │
╰──────────────────────┴─────────────────────────────────────────────────────────────────────╯
```

## `rsconstruct product`

**Requires config.**

Inspect a single product in the build graph: everything that goes into its cache key and whether it would be rebuilt. Use it to answer "why is this file rebuilding?".

| Subcommand | Config required? |
|------------|-----------------|
| `show` | Yes |

```bash
rsconstruct product show out/processor.generator.cc_single_file/src/main.elf  # By output path
rsconstruct product show src/main.c                                            # By primary input
```

The path is looked up as an output first; if no product owns it, products that have it as their primary input are shown. A leading `./` is ignored, so a shell-completed `./README.md` is the same query as `README.md`. The `hash_pieces` section is recomputed live by every analyzer that scans the product (each source of a multi-source product); an analyzer that fails to scan it is an error, not an empty section. Example output:

```text
processor: processor.generator.cc_single_file
outputs:
  out/processor.generator.cc_single_file/src/test.elf
inputs:
  primary src/test.c
  analyzer [icpp]
    src/utils.h
    src/config.h
cache_key: 7b1eccc2...
  config b884aa0c...
  tool 2a4d5d7d...
hash_pieces: (none)
input_checksum: 71e9982e...
descriptor_key: 8c2a5920...
cache_state: BUILD (no cache entry)
```

## `rsconstruct symlink-install`

**Requires config.** (no subcommands)

Symlink every file under one or more source folders into target folders, recreating the directory structure. Configure the folder pairs in `rsconstruct.toml`; `sources[i]` is installed into `targets[i]`, so both arrays must have the same length. A leading `~/` in a target is expanded to `$HOME`.

```toml
[command.symlink_install]
sources = ["dotfiles", "bin"]
targets = ["~/", "~/.local/bin"]
```

```bash
rsconstruct symlink-install
```

```text
dotfiles → /home/user/
  created .bashrc
  created .config/git/config
Symlink install: 2 created, 0 updated, 0 unchanged
```

Each link points to the absolute path of the source file. Running it again is safe: links that already point to the right file are left alone (`unchanged`), and links pointing elsewhere are replaced (`updated`). **A regular file at a target path is deleted and replaced by the link**, so make sure the target folders hold nothing you want to keep under the same names.

## `rsconstruct version`

**No config needed.** (no subcommands)

Print version information.

```bash
rsconstruct version
```
