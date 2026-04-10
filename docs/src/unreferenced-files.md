# Unreferenced Files

## Purpose

Find files on disk that are not referenced by any product in the build graph.
This helps identify forgotten assets, stale files, or files accidentally excluded
from the build configuration.

## How It Works

When rsconstruct builds its graph, every product has an `inputs` list. This list
contains **all** files the product depends on:

- **Primary inputs** — the source files being processed (e.g. `foo.svg` that
  mermaid converts to a PNG)
- **Dependency inputs** — files that affect the output but are not the primary
  source (e.g. a C header file `utils.h` that `main.c` includes, a config file
  like `.ruff.toml`, or a script passed via `dep_inputs`)

A file is **unreferenced** if it does not appear in the `inputs` list of any
product in the graph — neither as a primary input nor as a dependency input.

### Why both primary and dependency inputs?

Consider a C header file `utils.h`. It is not a primary input (the compiler does
not produce output directly from it), but it appears in `dep_inputs` because
changes to it must trigger a rebuild of any `.c` file that includes it. Such a
file is clearly referenced and should not be reported as unreferenced.

Only files that appear in **no** product's inputs list — not primary, not
dependency — are reported.

## Usage

```
rsconstruct graph unreferenced --extensions .svg[,.png,...] [--rm]
```

### Options

| Option | Description |
|--------|-------------|
| `--extensions` | Comma-separated list of file extensions to check (required) |
| `--rm` | Delete the unreferenced files immediately (no confirmation) |

### Examples

Find unreferenced SVG files:
```
rsconstruct graph unreferenced --extensions .svg
```

Find unreferenced images of any type:
```
rsconstruct graph unreferenced --extensions .svg,.png,.jpg
```

Delete unreferenced SVG files:
```
rsconstruct graph unreferenced --extensions .svg --rm
```

## Output

Plain list of file paths, one per line, relative to the project root:

```
assets/old_diagram.svg
docs/unused_figure.svg
scratch/test.svg
```

## Design Notes

- Extensions are **required** — defaulting to all files would produce excessive
  noise (READMEs, Makefiles, config files, etc. are intentionally not in the
  graph).
- Finding unreferenced files does not mean they are useless. The user decides
  what to do. Common reasons a file might be unreferenced:
  - It was part of a processor whose `src_dirs` or `src_extensions` excludes it
  - It was intentionally left out of the build
  - It is a leftover from a renamed or deleted processor instance
  - It is a scratch/draft file
- `--rm` deletes without confirmation. Use with care.
- The command requires a `rsconstruct.toml` (the graph must be buildable).
