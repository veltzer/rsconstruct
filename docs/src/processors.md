# Processors

RSConstruct uses **processors** to discover and build products. Each processor scans for source files matching its conventions and produces output files.

## Processor Types

Processors are classified into three types:

- **Generators** — produce real output files from input files (e.g., compiling code, rendering templates, transforming file formats)
- **Checkers** — validate input files without producing output files (e.g., linters, spell checkers, static analyzers). Success is recorded in the cache database.
- **Mass generators** — produce a directory of output files without enumerating them individually (e.g., sphinx, mdbook, cargo, pip, npm, gem). Output directories can be cached and restored as a whole — see [Output directory caching](#output-directory-caching) below.

The processor type is displayed in `rsconstruct processors list` output:

```
cc_single_file [generator] enabled
ruff [checker] enabled
tera [generator] enabled
```

For checkers, `rsconstruct processors files` shows "(checker)" instead of output paths since no files are produced:

```
[ruff] (3 products)
src/foo.py → (checker)
src/bar.py → (checker)
```

## Configuration

Enable processors in `rsconstruct.toml`:

```toml
[processor]
enabled = ["tera", "ruff", "pylint", "pyrefly", "cc_single_file", "cppcheck", "shellcheck", "spellcheck", "make", "yamllint", "jq", "jsonlint", "taplo", "json_schema"]
```

Use `rsconstruct processors list` to see available processors with enabled/detected status and descriptions.
Use `rsconstruct processors list --all` to include hidden processors.
Use `rsconstruct processors files` to see which files each processor discovers.

## Available Processors

- [Tera](processors/tera.md) — renders Tera templates into output files
- [Ruff](processors/ruff.md) — lints Python files with ruff
- [Pylint](processors/pylint.md) — lints Python files with pylint
- [Mypy](processors/mypy.md) — type-checks Python files with mypy
- [Pyrefly](processors/pyrefly.md) — type-checks Python files with pyrefly
- [CC](processors/cc.md) — builds full C/C++ projects from cc.yaml manifests
- [CC Single File](processors/cc_single_file.md) — compiles C/C++ source files into executables (single-file)
- [Linux Module](processors/linux_module.md) — builds Linux kernel modules from linux-module.yaml manifests
- [Cppcheck](processors/cppcheck.md) — runs static analysis on C/C++ source files
- [Clang-Tidy](processors/clang_tidy.md) — runs clang-tidy static analysis on C/C++ source files
- [Shellcheck](processors/shellcheck.md) — lints shell scripts using shellcheck
- [Spellcheck](processors/spellcheck.md) — checks documentation files for spelling errors
- [Rumdl](processors/rumdl.md) — lints Markdown files with rumdl
- [Make](processors/make.md) — runs make in directories containing Makefiles
- [Cargo](processors/cargo.md) — builds Rust projects using Cargo
- [Yamllint](processors/yamllint.md) — lints YAML files with yamllint
- [Jq](processors/jq.md) — validates JSON files with jq
- [Jsonlint](processors/jsonlint.md) — lints JSON files with jsonlint
- [Taplo](processors/taplo.md) — checks TOML files with taplo
- [Json Schema](processors/json_schema.md) — validates JSON schema propertyOrdering

## Output Directory Caching

Mass generators (sphinx, mdbook, cargo, pip, npm, gem) produce output in directories
rather than individual files. RSConstruct can cache these entire directories so that after
`rsconstruct clean && rsconstruct build`, the output is restored from cache instead of being regenerated.

After a successful build, RSConstruct walks the output directory, stores every file as a
content-addressed blob in `.rsconstruct/objects/`, and records a manifest (path, checksum,
Unix permissions) in the cache entry. On restore, the entire directory is recreated
from cached objects with permissions preserved.

This is controlled by the `cache_output_dir` config option (default `true`) on each
mass generator:

```toml
[processor.sphinx]
cache_output_dir = true    # Cache _build/ directory (default)

[processor.cargo]
cache_output_dir = false   # Disable for large target/ directories
```

Output directories cached per processor:

| Processor | Output directory |
|-----------|-----------------|
| sphinx    | `_build` (configurable via `output_dir`) |
| mdbook    | `book` (configurable via `output_dir`) |
| cargo     | `target` |
| pip       | `out/pip` |
| npm       | `node_modules` |
| gem       | `vendor/bundle` |

When `cache_output_dir` is `false`, the processor falls back to the previous behavior
(stamp-file or empty-output caching, no directory restore).

## Custom Processors

You can define custom processors in Lua. See [Lua Plugins](plugins.md) for details.
