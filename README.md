# RSB - Rust Build Tool

A fast, incremental build tool written in Rust with C/C++ compilation, template support, Python linting, and parallel execution.

## Features

- **Incremental builds** using SHA-256 checksums to detect changes
- **C/C++ compilation** with automatic header dependency tracking
- **Parallel execution** of independent build products with `-j` flag
- **Template processing** via the Tera templating engine
- **Python linting** with ruff (configurable)
- **Deterministic builds** — same input always produces same build order
- **Graceful interrupt** — Ctrl+C saves progress, next build resumes where it left off
- **Convention over configuration** — simple naming conventions, minimal config needed

## Installation

```bash
cargo build --release
```

## Usage

```bash
rsb build                    # Incremental build
rsb build --force            # Force full rebuild
rsb build -j4                # Build with 4 parallel jobs
rsb build --processor-verbose 2  # Show source paths in output
rsb clean                    # Remove build artifacts and cache
rsb status                   # Show what needs rebuilding
rsb graph                    # Print dependency graph (formats: dot, mermaid, json, text)
rsb graph --view             # Open graph in browser (mermaid) or as SVG (dot)
rsb watch                    # Watch for changes and rebuild automatically
rsb init                     # Create a new project
rsb complete [shell]         # Generate shell completions
```

## Configuration (rsb.toml)

```toml
[build]
parallel = 1  # Number of parallel jobs (1 = sequential, 0 = auto-detect CPU cores)

[processor]
enabled = ["cc", "template", "pylint", "sleep", "cpplint"]

[cache]
restore_method = "hardlink"  # or "copy" (hardlink is faster, copy works across filesystems)

[processor.cc]
cc = "gcc"              # C compiler (default: gcc)
cxx = "g++"             # C++ compiler (default: g++)
cflags = ["-Wall"]      # C compiler flags
cxxflags = ["-Wall"]    # C++ compiler flags
ldflags = []            # Linker flags
include_paths = ["src/include"]  # Additional -I paths (passed as-is)
source_dir = "src"      # Source directory (default: src)
output_suffix = ".elf"  # Suffix for output executables (default: .elf)

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

[completions]
shells = ["bash"]
```

## Project Structure

```
project/
├── rsb.toml          # Configuration file
├── config/           # Python config files
├── templates/        # .tera template files
├── src/              # C/C++ source files
├── sleep/            # .sleep files (for parallel testing)
├── out/
│   ├── cc/           # Compiled executables
│   ├── pylint/       # Python lint stub files
│   ├── cpplint/      # C/C++ lint stub files
│   └── sleep/        # Sleep stub files
└── .rsb/             # Cache (index.json, objects/, deps/)
```

## C/C++ Processor

The cc processor compiles C (`.c`) and C++ (`.cc`) source files into executables under `out/cc/`, mirroring the source directory structure: `src/a/b.c` → `out/cc/a/b.elf`.

Header dependencies are automatically tracked via `gcc -MM`, so changes to included headers trigger recompilation.

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

- `EXTRA_*_FLAGS_*` — literal flags (with backtick expansion for command substitution)
- `EXTRA_*_CMD` — executed as subprocess (no shell), stdout used as flags
- `EXTRA_*_SHELL` — executed via `sh -c` (full shell syntax), stdout used as flags

### Command line ordering

```
compiler -MMD -MF deps -I... [compile_before] [cflags/cxxflags] [compile_after] -o output source [link_before] [ldflags] [link_after]
```

Link flags come after the source file so the linker can resolve symbols correctly.

## Templates

Files matching configured extensions in `templates/` generate output files in the project root. Default: `templates/{X}.tera` → `{X}`.

Templates can load Python configuration using the built-in `load_python()` function:

```jinja2
{% set config = load_python(path="config/settings.py") %}
[app]
name = "{{ config.project_name }}"
version = "{{ config.version }}"
```

## Verbosity Levels (`--processor-verbose N`)

- **0** (default) — target basename only: `main.elf`
- **1** — target path (relative to cwd): `out/cc/main.elf`; cc processor also prints compiler commands
- **2** — adds source file path: `out/cc/main.elf <- src/main.c`
- **3** — adds all inputs including headers: `out/cc/main.elf <- src/main.c, src/utils.h`

## Ignoring Files

Create a `.rsbignore` file in the project root with glob patterns (one per line) to exclude files from processing:

```
/src/experiments/**
*.bak
```
