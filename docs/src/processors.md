# Processors

RSB uses **processors** to discover and build products. Each processor scans for source files matching its conventions and produces output files.

## Processor Types

Processors are classified into two types:

- **Generators** — produce real output files from input files (e.g., compiling code, rendering templates, transforming file formats)
- **Checkers** — validate input files without producing output files (e.g., linters, spell checkers, static analyzers). Success is recorded in the cache database.

The processor type is displayed in `rsb processors list` output:

```
cc_single_file [generator] enabled
ruff [checker] enabled
tera [generator] enabled
```

For checkers, `rsb processors files` shows "(checker)" instead of output paths since no files are produced:

```
[ruff] (3 products)
src/foo.py → (checker)
src/bar.py → (checker)
```

## Configuration

Enable processors in `rsb.toml`:

```toml
[processor]
enabled = ["tera", "ruff", "pylint", "pyrefly", "cc_single_file", "cppcheck", "shellcheck", "spellcheck", "make", "yamllint", "jq", "jsonlint", "taplo", "json_schema"]
```

Use `rsb processors list` to see available processors with enabled/detected status and descriptions.
Use `rsb processors list --all` to include hidden processors.
Use `rsb processors files` to see which files each processor discovers.

## Available Processors

- [Tera](processors/tera.md) — renders Tera templates into output files
- [Ruff](processors/ruff.md) — lints Python files with ruff
- [Pylint](processors/pylint.md) — lints Python files with pylint
- [Mypy](processors/mypy.md) — type-checks Python files with mypy
- [Pyrefly](processors/pyrefly.md) — type-checks Python files with pyrefly
- [CC Single File](processors/cc.md) — compiles C/C++ source files into executables (single-file)
- [Cppcheck](processors/cppcheck.md) — runs static analysis on C/C++ source files
- [Clang-Tidy](processors/clang_tidy.md) — runs clang-tidy static analysis on C/C++ source files
- [Shellcheck](processors/shellcheck.md) — lints shell scripts using shellcheck
- [Spellcheck](processors/spellcheck.md) — checks documentation files for spelling errors
- [Rumdl](processors/rumdl.md) — lints Markdown files with rumdl
- [Sleep](processors/sleep.md) — sleeps for a duration (for testing)
- [Make](processors/make.md) — runs make in directories containing Makefiles
- [Cargo](processors/cargo.md) — builds Rust projects using Cargo
- [Yamllint](processors/yamllint.md) — lints YAML files with yamllint
- [Jq](processors/jq.md) — validates JSON files with jq
- [Jsonlint](processors/jsonlint.md) — lints JSON files with jsonlint
- [Taplo](processors/taplo.md) — checks TOML files with taplo
- [Json Schema](processors/json_schema.md) — validates JSON schema propertyOrdering

## Custom Processors

You can define custom processors in Lua. See [Lua Plugins](plugins.md) for details.
