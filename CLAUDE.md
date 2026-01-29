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
- `rsb processor list` - List available processors and their status
- `rsb complete [shell]` - Generate shell completions
- `rsb version` - Print version information

## Configuration (rsb.toml)

```toml
[build]
parallel = 1  # Number of parallel jobs (1 = sequential, 0 = auto-detect CPU cores)

[processor]
enabled = ["template", "pylint", "sleep", "cc", "cpplint", "spellcheck"]

[cache]
restore_method = "hardlink"  # or "copy" (hardlink is faster, copy works across filesystems)

[processor.template]
strict = true           # Fail on undefined variables (default: true)
extensions = [".tera"]  # File extensions to process
trim_blocks = false     # Remove newline after block tags

[processor.pylint]
linter = "ruff"
args = []

[processor.cpplint]
checker = "cppcheck"  # C/C++ static checker (default: cppcheck)
args = ["--error-exitcode=1", "--enable=warning,style,performance,portability"]
# To use a suppressions file: add "--suppressions-list=.cppcheck-suppressions" to args

[processor.spellcheck]
extensions = [".md"]                    # File extensions to check
language = "en_US"                      # Hunspell dictionary language
words_file = ".spellcheck-words"        # Path to custom words file (relative to project root)

[processor.cc]
cc = "gcc"              # C compiler (default: gcc)
cxx = "g++"             # C++ compiler (default: g++)
cflags = []             # C compiler flags
cxxflags = []           # C++ compiler flags
ldflags = []            # Linker flags
include_paths = []      # Additional -I paths (relative to project root)
source_dir = "src"      # Source directory (default: src)
output_suffix = ".elf"  # Suffix for output executables (default: .elf)

[graph]
viewer = "google-chrome"  # Command to open graph files (default: platform-specific)

[completions]
shells = ["bash"]
```

## Project Structure

```
project/
├── rsb.toml              # Configuration file
├── .spellcheck-words     # Custom words for spellcheck (one per line)
├── config/               # Python config files
├── templates/        # .tera template files
├── src/              # C/C++ source files
├── sleep/            # .sleep files (for parallel testing)
├── out/
│   ├── cc/           # Compiled executables
│   ├── pylint/       # Python lint stub files
│   ├── cpplint/      # C/C++ lint stub files
│   ├── spellcheck/   # Spellcheck stub files
│   └── sleep/        # Sleep stub files
└── .rsb/             # Cache (index.json, objects/, deps/)
```

## C/C++ Processor

The cc processor compiles C (`.c`) and C++ (`.cc`) source files from the source directory into executables under `out/cc/`, mirroring the directory structure. Source files: `src/a/b.c` → `out/cc/a/b.elf`.

### Per-file flags

Per-file compile and link flags can be set via comments in source files:

```c
// EXTRA_COMPILE_FLAGS_BEFORE=-pthread
// EXTRA_COMPILE_FLAGS_AFTER=-O2 -DNDEBUG
// EXTRA_LINK_FLAGS_BEFORE=-L/usr/local/lib
// EXTRA_LINK_FLAGS_AFTER=-lX11
// EXTRA_COMPILE_CMD=pkg-config --cflags gtk+-3.0
// EXTRA_LINK_CMD=pkg-config --libs gtk+-3.0
// EXTRA_COMPILE_SHELL=echo -DLEVEL2_CACHE_LINESIZE=$(getconf LEVEL2_CACHE_LINESIZE)
// EXTRA_LINK_SHELL=echo -L$(brew --prefix openssl)/lib
```

Supported comment styles: `//`, `/* ... */` (single-line), and `*`-prefixed block comment continuation lines:

```c
/*
 * EXTRA_LINK_FLAGS_AFTER=-lX11
 */
```

`EXTRA_*_FLAGS_*` values are literal flags (with backtick expansion for command substitution).
`EXTRA_*_CMD` values are executed as subprocesses (no shell) and stdout is used as flags.
`EXTRA_*_SHELL` values are executed via `sh -c` (full shell syntax) and stdout is used as flags.

### Command line ordering

The compiler command is constructed in this order:

```
compiler -MMD -MF deps -I... [compile_before] [cflags/cxxflags] [compile_after] -o output source [link_before] [ldflags] [link_after]
```

Link flags come after the source file so the linker can resolve symbols correctly.

### Processor verbosity levels (`--processor-verbose N`)

- **0** (default) — target basename only: `main.elf`
- **1** — target path (relative to cwd): `out/cc/main.elf`; cc processor also prints compiler commands
- **2** — adds source file path: `out/cc/main.elf <- src/main.c`
- **3** — adds all inputs including headers: `out/cc/main.elf <- src/main.c, src/utils.h`

## Spellcheck Processor

The spellcheck processor checks documentation files for spelling errors using the `zspell` crate (pure Rust, compatible with Hunspell dictionary format). It reads system Hunspell dictionaries from `/usr/share/hunspell/`.

- Default file extensions: `.md`
- Default language: `en_US` (requires `hunspell-en-us` package or equivalent)
- Custom words file: `.spellcheck-words` at project root (one word per line, `#` comments supported)
- Markdown-aware: strips code blocks, inline code, URLs, and HTML tags before checking
- Output: stub files in `out/spellcheck/`

## Architecture

- **Processors** implement `ProductDiscovery` trait (template, pylint, sleep, cc, cpplint, spellcheck)
- **Products** have inputs (source files) and outputs (generated files)
- **BuildGraph** manages dependencies between products
- **Executor** runs products in dependency order, with optional parallelism
- **Build order** is deterministic — file discovery, processor iteration, and topological sort are all sorted
- **Config-aware caching** — processor config (compiler flags, linter args, etc.) is hashed into cache keys so config changes trigger rebuilds

## How Templates Work

- Files matching configured extensions in `templates/` generate output files in project root
- Default: `templates/{X}.tera` → `{X}`
- Templates use `load_python(path="config/settings.py")` to load Python variables

## Philosophy

Convention over configuration - simple naming conventions, explicit config loading, incremental builds by default.
