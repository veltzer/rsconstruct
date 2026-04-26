# Processors

RSConstruct uses **processors** to discover and build products. Each processor scans for source files matching its conventions and produces output files.

## Processor Types

There are four processor types: checker, generator, creator, and explicit. They differ in how inputs are discovered, how outputs are declared, and how results are cached.

See [Processor Types](processor-types.md) for full descriptions, examples, and a comparison table.

## Configuration

Declare processors by adding `[processor.NAME]` sections to `rsconstruct.toml`:

```toml
[processor.ruff]

[processor.pylint]
args = ["--disable=C0114"]

[processor.cc_single_file]
```

Only declared processors run — no processors are enabled by default. Use `rsconstruct smart auto` to auto-detect and add relevant processors.

Use `rsconstruct processors list` to see declared processors and descriptions.
Use `rsconstruct processors list --all` to show all built-in processors, not just those enabled in the project.
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
- [Zspell](processors/zspell.md) — checks documentation files for spelling errors
- [Rumdl](processors/rumdl.md) — lints Markdown files with rumdl
- [Make](processors/make.md) — runs make in directories containing Makefiles
- [Cargo](processors/cargo.md) — builds Rust projects using Cargo
- [Yamllint](processors/yamllint.md) — lints YAML files with yamllint
- [Jq](processors/jq.md) — validates JSON files with jq
- [Jsonlint](processors/jsonlint.md) — lints JSON files with jsonlint
- [Taplo](processors/taplo.md) — checks TOML files with taplo
- [Terms](processors/terms.md) — checks that technical terms are backtick-quoted in Markdown files
- [Json Schema](processors/json_schema.md) — validates JSON schema propertyOrdering
- [Iyamlschema](processors/iyamlschema.md) — validates YAML files against JSON schemas (native)
- [Yaml2json](processors/yaml2json.md) — converts YAML files to JSON (native)
- [Markdown2html](processors/markdown2html.md) — converts Markdown to HTML using markdown CLI
- [Imarkdown2html](processors/imarkdown2html.md) — converts Markdown to HTML (native)

## Output Directory Caching

Creator processors (cargo, sphinx, mdbook, pip, npm, gem, and user-defined creators) produce output in directories rather than individual files. RSConstruct caches these entire directories so that after `rsconstruct clean && rsconstruct build`, the output is restored from cache instead of being regenerated.

After a successful build, RSConstruct walks the output directories, stores every file as a content-addressed blob, and records a tree (manifest of paths, checksums, and Unix permissions). On restore, the entire directory tree is recreated from cached blobs with permissions preserved. See [Cache System](internal/cache.md) for details.

For user-defined creators, output directories are declared via `output_dirs`:

```toml
[processor.creator.venv]
command = "pip"
args = ["install", "-r", "requirements.txt"]
src_extensions = ["requirements.txt"]
output_dirs = [".venv"]
```

For built-in creators, this is controlled by the `cache_output_dir` config option (default `true`):

```toml
[processor.cargo]
cache_output_dir = false   # Disable for large target/ directories
```

## Clean behavior

`rsconstruct clean outputs` calls each product's processor-defined `clean()`. What gets removed depends on the processor type:

| Processor type | What `clean()` removes | Recursive? |
|---|---|---|
| Checker | Nothing — checkers produce no outputs | — |
| Generator | Each declared output file (`product.outputs`) | No |
| Creator | Each declared output directory (`product.output_dirs`) | **Yes** |
| Explicit | Declared `output_files` + declared `output_dirs` | Yes (for `output_dirs` only) |
| Lua plugin | Custom `clean()` if defined; otherwise file-only | No (unless plugin code does it) |

Recursive directory removal is reserved for Creators (whose external build tool produces an unknown set of files inside a known directory) and the user-declared `output_dirs` of Explicit. All other processor types remove individually-declared files and nothing else.

After every product's `clean()` completes, the orchestrator runs an empty-directory sweep: every parent of every removed output (and every parent of every removed `output_dir`) is tried with `fs::remove_dir` (non-recursive — succeeds only on already-empty directories), walking upward until a non-empty directory or the project root. The sweep does not special-case `out/` — any ancestor of a cleaned output is eligible.

See [`rsconstruct clean`](commands.md#rsconstruct-clean) for the full command reference, the `--no-empty-dirs` flag, the `-p` filter, and the other clean variants (`all`, `git`, `unknown`).

### Worked example

Consider this project:

```toml
# rsconstruct.toml

[processor.ruff]                     # Checker — no outputs
src_dirs = ["src"]

[processor.tera]                     # Generator — declared file outputs
src_dirs = ["templates"]
src_extensions = [".tera"]

[processor.mdbook]                   # Creator — declared output_dir
src_dirs = ["docs"]
```

Layout:

```
project/
├── rsconstruct.toml
├── src/main.py                      # ruff lints this
├── templates/index.html.tera        # tera renders to → out/tera/index.html
├── docs/
│   ├── book.toml
│   └── src/SUMMARY.md
└── book/                            # mdbook output_dir; contents unenumerated
    ├── index.html
    ├── css/main.css
    └── ... (many files)
```

After `rsconstruct build`, both `out/tera/index.html` and the entire `book/` tree exist.

Now run `rsconstruct clean outputs`:

1. **ruff** — Checker. `clean()` is a no-op. Nothing removed.
2. **tera** — Generator. `clean()` calls `fs::remove_file("out/tera/index.html")`. The file goes; `out/tera/` and `out/` are not yet touched.
3. **mdbook** — Creator. `clean()` calls `fs::remove_dir_all("book/")`. The whole `book/` tree (including `book/css/main.css` and everything else) is removed. `book/`'s parent (the project root) is not touched.
4. **Empty-directory sweep.** The orchestrator collects parents of every removed path: `out/tera`, `out` (parents of the tera output) and `.` (parent of `book/`, which is the project root and gets ignored). Sorted deepest-first: `out/tera`, `out`. `fs::remove_dir("out/tera")` → succeeds (empty). `fs::remove_dir("out")` → succeeds (now empty). Project root is skipped.

Final state: `src/main.py`, `templates/index.html.tera`, `docs/...`, and `rsconstruct.toml` remain untouched. The cache (`.rsconstruct/`) is untouched. `out/` and `book/` are gone.

If you instead ran `rsconstruct clean outputs --no-empty-dirs`, the only difference is step 4 is skipped — `out/tera/` and `out/` would remain as empty directories.

If you ran `rsconstruct clean outputs -p tera`, only step 2 + the sweep of its parents runs. `book/` is left intact because mdbook is filtered out.

## Custom Processors

You can define custom processors in Lua. See [Lua Plugins](plugins.md) for details.
