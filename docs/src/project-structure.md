# Project Structure

RSConstruct follows a convention-over-configuration approach. The directory layout determines how files are processed.

## Directory layout

```
project/
├── rsconstruct.toml          # Configuration file
├── .rsconstructignore        # Glob patterns for files to exclude
├── config/           # Python config files (loaded by templates)
├── templates.tera/   # .tera template files
├── templates.mako/   # .mako template files
├── src/              # C/C++ source files
├── plugins/          # Lua processor plugins (.lua files)
├── out/
│   ├── cc_single_file/ # Compiled executables
│   ├── ruff/         # Ruff lint stub files
│   ├── pylint/       # Pylint lint stub files
│   ├── cppcheck/      # C/C++ lint stub files
│   ├── spellcheck/   # Spellcheck stub files
│   ├── sleep/        # Sleep stub files
│   └── make/         # Make stub files
└── .rsconstruct/             # Cache directory
    ├── index.json    # Cache index
    ├── objects/       # Cached build artifacts
    └── deps/          # Dependency files
```

## Conventions

### Templates

Files in `templates.tera/` with configured extensions (default `.tera`) are rendered to the project root:

- `templates.tera/Makefile.tera` produces `Makefile`
- `templates.tera/config.toml.tera` produces `config.toml`

Similarly, files in `templates.mako/` with `.mako` extensions are rendered via the Mako processor:

- `templates.mako/Makefile.mako` produces `Makefile`
- `templates.mako/config.toml.mako` produces `config.toml`

### C/C++ sources

Files in the source directory (default `src/`) are compiled to executables under `out/cc_single_file/`, preserving the directory structure:

- `src/main.c` produces `out/cc_single_file/main.elf`
- `src/utils/helper.cc` produces `out/cc_single_file/utils/helper.elf`

### Python files

Python files are linted and stub outputs are written to `out/ruff/` (ruff processor) or `out/pylint/` (pylint processor).

### Build artifacts

All build outputs go into `out/`. The cache lives in `.rsconstruct/`. Use `rsconstruct clean` to remove `out/` (preserving cache) or `rsconstruct clean all` to remove both.
