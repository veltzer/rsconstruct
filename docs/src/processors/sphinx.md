# Sphinx Processor

## Purpose

Builds Sphinx documentation projects.

## How It Works

Discovers `conf.py` files indicating Sphinx projects, collects sibling `.rst`,
`.py`, and `.md` files as inputs, and runs `sphinx-build` to generate output.
A non-zero exit code fails the product.

## Source Files

- Input: `**/conf.py` (plus sibling `.rst`, `.py`, `.md` files)
- Output: none (mass_generator — produces output in `_build` directory)

## Configuration

```toml
[processor.sphinx]
sphinx_build = "sphinx-build"          # The sphinx-build command to run
output_dir = "_build"                  # Output directory for generated docs
args = []                              # Additional arguments to pass to sphinx-build
extra_inputs = []                      # Additional files that trigger rebuilds when changed
cache_output_dir = true                # Cache the output directory for fast restore after clean
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `sphinx_build` | string | `"sphinx-build"` | The sphinx-build executable to run |
| `output_dir` | string | `"_build"` | Output directory for generated documentation |
| `args` | string[] | `[]` | Extra arguments passed to sphinx-build |
| `extra_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |
| `cache_output_dir` | boolean | `true` | Cache the `_build/` directory so `rsbuild clean && rsbuild build` restores from cache |
