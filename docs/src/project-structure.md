# Project Structure

RSConstruct follows a convention-over-configuration approach. The directory layout determines how files are processed.

## Directory layout

```text
project/
├── rsconstruct.toml          # Configuration file
├── .rsconstructignore        # Glob patterns for files to exclude
├── config/           # Python config files (loaded by templates)
├── tera.templates/   # .tera template files
├── templates.mako/   # .mako template files
├── src/              # C/C++ source files
├── out/
│   └── processor.generator.cc_single_file/   # Compiled executables (one directory per generating processor, named after it)
└── .rsconstruct/             # Cache directory
    ├── db.redb       # Cache index (processor configs, product -> cached tree)
    ├── descriptors/  # Per-product descriptors: which cached objects make up each product's outputs
    ├── objects/      # Content-addressed cached build artifacts
    ├── deps.redb     # Dependency cache (analyzer results)
    └── mtime.redb    # Persistent mtime checksum cache
```

Checkers (ruff, pylint, cppcheck, zspell, make, ...) write nothing under `out/`; only generators and the other output-producing types get a directory there.

## Conventions

### Templates

Files in `tera.templates/` with configured extensions (default `.tera`) are rendered to the project root:

- `tera.templates/Makefile.tera` produces `Makefile`
- `tera.templates/config.toml.tera` produces `config.toml`

Similarly, files in `templates.mako/` with `.mako` extensions are rendered via the Mako processor:

- `templates.mako/Makefile.mako` produces `Makefile`
- `templates.mako/config.toml.mako` produces `config.toml`

### C/C++ sources

Files in the configured source directories (`src_dirs`, e.g. `["src"]`) are compiled to executables under `out/processor.generator.cc_single_file/`, preserving the directory structure:

- `src/main.c` produces `out/processor.generator.cc_single_file/src/main.elf`
- `src/utils/helper.cc` produces `out/processor.generator.cc_single_file/src/utils/helper.elf`

### Python files

Python files are linted by `processor.checker.ruff` or `processor.checker.pylint`. Checkers write no files: a passing check is recorded in the cache, so an unchanged file is not checked again.

### Build artifacts

All build outputs go into `out/`. The cache lives in `.rsconstruct/`. Use `rsconstruct clean outputs` to remove every declared output (preserving the cache) — directories left empty afterward are also pruned, so `out/` typically disappears too. Use `rsconstruct clean all` to remove `out/` and `.rsconstruct/` outright. See [`rsconstruct clean`](commands.md#rsconstruct-clean) for the full breakdown of clean behavior.
