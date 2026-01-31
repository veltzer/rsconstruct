# Project Structure

RSB follows a convention-over-configuration approach. The directory layout determines how files are processed.

## Directory layout

```
project/
├── rsb.toml          # Configuration file
├── .rsbignore        # Glob patterns for files to exclude
├── config/           # Python config files (loaded by templates)
├── templates/        # .tera template files
├── src/              # C/C++ source files
├── out/
│   ├── cc_single_file/ # Compiled executables
│   ├── ruff/         # Ruff lint stub files
│   ├── pylint/       # Pylint lint stub files
│   ├── cpplint/      # C/C++ lint stub files
│   ├── spellcheck/   # Spellcheck stub files
│   ├── sleep/        # Sleep stub files
│   └── make/         # Make stub files
└── .rsb/             # Cache directory
    ├── index.json    # Cache index
    ├── objects/       # Cached build artifacts
    └── deps/          # Dependency files
```

## Conventions

### Templates

Files in `templates/` with configured extensions (default `.tera`) are rendered to the project root:

- `templates/Makefile.tera` produces `Makefile`
- `templates/config.toml.tera` produces `config.toml`

### C/C++ sources

Files in the source directory (default `src/`) are compiled to executables under `out/cc_single_file/`, preserving the directory structure:

- `src/main.c` produces `out/cc_single_file/main.elf`
- `src/utils/helper.cc` produces `out/cc_single_file/utils/helper.elf`

### Python files

Python files are linted and stub outputs are written to `out/ruff/` (ruff processor) or `out/pylint/` (pylint processor).

### Build artifacts

All build outputs go into `out/`. The cache lives in `.rsb/`. Use `rsb clean` to remove `out/` (preserving cache) or `rsb distclean` to remove both.
