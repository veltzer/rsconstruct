# Processors

RSConstruct uses **processors** to discover and build products. Each processor scans for source files matching its conventions and produces output files.

## Processor Types

There are four processor types: checker, generator, explicit, and mass generator. They differ in how inputs are discovered, how outputs are declared, and how results are cached.

See [Processor Types](processor-types.md) for full descriptions, examples, and a comparison table.

## Configuration

Declare processors by adding `[processor.TYPE.NAME]` sections to `rsconstruct.toml`:

```toml
[processor.checker.ruff]

[processor.checker.pylint]
args = ["--disable=C0114"]

[processor.generator.cc_single_file]
```

Only declared processors run — no processors are enabled by default. Use `rsconstruct smart auto` to auto-detect and add relevant processors.

Use `rsconstruct processor list` to see declared processors and descriptions.
Use `rsconstruct processor list --all` to show all built-in processors, not just those enabled in the project.
Use `rsconstruct processor files` to see which files each processor discovers.

## Available Processors

- [Tera](processor/generator/tera.md) — renders Tera templates into output files
- [Ruff](processor/checker/ruff.md) — lints Python files with ruff
- [Pylint](processor/checker/pylint.md) — lints Python files with pylint
- [Mypy](processor/checker/mypy.md) — type-checks Python files with mypy
- [Pyrefly](processor/checker/pyrefly.md) — type-checks Python files with pyrefly
- [CC](processor/mass_generator/cc.md) — builds full C/C++ projects from cc.yaml manifests
- [CC Single File](processor/generator/cc_single_file.md) — compiles C/C++ source files into executables (single-file)
- [Linux Module](processor/mass_generator/linux_module.md) — builds Linux kernel modules from linux-module.yaml manifests
- [Cppcheck](processor/checker/cppcheck.md) — runs static analysis on C/C++ source files
- [Clang-Tidy](processor/checker/clang_tidy.md) — runs clang-tidy static analysis on C/C++ source files
- [Shellcheck](processor/checker/shellcheck.md) — lints shell scripts using shellcheck
- [Zspell](processor/checker/zspell.md) — checks documentation files for spelling errors
- [Rumdl](processor/checker/rumdl.md) — lints Markdown files with rumdl
- [Oxlint](processor/checker/oxlint.md) — lints JavaScript/TypeScript files with oxlint
- [Selene](processor/checker/selene.md) — lints Lua files with selene
- [Iluacheck](processor/checker/iluacheck.md) — lints Lua files with luacheck's analysis (builtin)
- [Biome](processor/checker/biome.md) — lints CSS/JavaScript/TypeScript/JSON files with biome
- [Make](processor/checker/make.md) — runs make in directories containing Makefiles
- [Cargo](processor/mass_generator/cargo.md) — builds Rust projects using Cargo
- [Yamllint](processor/checker/yamllint.md) — lints YAML files with yamllint
- [Jq](processor/checker/jq.md) — validates JSON files with jq
- [Jsonlint](processor/checker/jsonlint.md) — lints JSON files with jsonlint
- [Taplo](processor/checker/taplo.md) — checks TOML files with taplo
- [Terms](processor/checker/terms.md) — checks that technical terms are backtick-quoted in Markdown files
- [Json Schema](processor/checker/json_schema.md) — validates JSON schema propertyOrdering
- [Iyamlschema](processor/checker/iyamlschema.md) — validates YAML files against JSON schemas (builtin)
- [Yaml2json](processor/generator/yaml2json.md) — converts YAML files to JSON (builtin)
- [Markdown2html](processor/generator/markdown2html.md) — converts Markdown to HTML using markdown CLI
- [Imarkdown2html](processor/generator/imarkdown2html.md) — converts Markdown to HTML (builtin)
- [Mass Generator](processor/mass_generator/generic.md) — runs a tool that enumerates its outputs in advance, one cached product per predicted file
- [Zola](processor/mass_generator/zola.md) — builds a zola site, one cached product per file zola writes, planned natively

## Clean behavior

`rsconstruct clean outputs` calls each product's processor-defined `clean()`. What gets removed depends on the processor type:

| Processor type | What `clean()` removes | Recursive? |
|---|---|---|
| Checker | Nothing — checkers produce no outputs | — |
| Generator | Each declared output file (`product.outputs`) | No |
| Explicit | Declared `output_files` + declared `output_dirs` | Yes (for `output_dirs` only) |
| Mass generator | Each predicted output file (`product.outputs`) | No |

Recursive directory removal is reserved for the user-declared `output_dirs` of Explicit. All other processor types remove individually-declared files and nothing else. A mass generator's `output_dirs` only bound where its predicted files may live; the directory itself is never removed directly, though the empty-directory sweep removes directories that become empty.

After every product's `clean()` completes, the orchestrator runs an empty-directory sweep: every parent of every removed output (and every parent of every removed `output_dir`) is tried with `fs::remove_dir` (non-recursive — succeeds only on already-empty directories), walking upward until a non-empty directory or the project root. The sweep does not special-case `out/` — any ancestor of a cleaned output is eligible.

See [`rsconstruct clean`](commands.md#rsconstruct-clean) for the full command reference, the `--no-empty-dirs` flag, the `-p` filter, and the other clean variants (`all`, `git`, `unknown`).

### Worked example

Consider this project:

```toml
# rsconstruct.toml

[processor.checker.ruff]                     # Checker — no outputs
src_dirs = ["src"]

[processor.generator.tera]                   # Generator — declared file outputs
src_dirs = ["templates"]
src_extensions = [".tera"]

[processor.mass_generator.mdbook]            # Mass generator — planned file outputs
src_dirs = ["docs"]
```

Layout:

```text
project/
├── rsconstruct.toml
├── src/main.py                      # ruff lints this
├── templates/index.html.tera        # tera renders to → out/processor.generator.tera/index.html
├── docs/
│   ├── book.toml
│   └── src/SUMMARY.md
└── book/                            # mdbook output_dir; each file is a planned product
    ├── index.html
    ├── css/general.css
    └── ...
```

After `rsconstruct build`, both `out/processor.generator.tera/index.html` and the planned files under `book/` exist.

Now run `rsconstruct clean outputs`:

1. **ruff** — Checker. `clean()` is a no-op. Nothing removed.
2. **tera** — Generator. `clean()` calls `fs::remove_file("out/processor.generator.tera/index.html")`. The file goes; `out/processor.generator.tera/` and `out/` are not yet touched.
3. **mdbook** — Mass generator. `clean()` removes each planned output file under `book/`.
4. **Empty-directory sweep.** The orchestrator collects parents of every removed path (`out/processor.generator.tera`, `out`, `book/css`, `book`) and removes those that are now empty, deepest-first.

Final state: `src/main.py`, `templates/index.html.tera`, `docs/...`, and `rsconstruct.toml` remain untouched. The cache (`.rsconstruct/`) is untouched. `out/` and `book/` are gone.

If you instead ran `rsconstruct clean outputs --no-empty-dirs`, the only difference is step 4 is skipped — `out/processor.generator.tera/` and `out/` would remain as empty directories.

If you ran `rsconstruct clean outputs -p processor.generator.tera`, only step 2 + the sweep of its parents runs. `book/` is left intact because mdbook is filtered out.

## Custom Processors

A command of your own runs through a config-driven processor: `[processor.checker.script]` for a check, `[processor.generator.generic]` for one output per input, `[processor.explicit.generic]` for declared inputs and outputs, and `[processor.mass_generator.generic]` for a tool that can predict its outputs. A new tool with its own discovery or output rules is a new processor file under `src/processor/`.
