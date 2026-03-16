# Mdbook Processor

## Purpose

Builds [mdbook](https://rust-lang.github.io/mdBook/) documentation projects.

## How It Works

Discovers `book.toml` files indicating mdbook projects, collects sibling `.md`
and `.toml` files as inputs, and runs `mdbook build`. A non-zero exit code
fails the product.

## Source Files

- Input: `**/book.toml` (plus sibling `.md`, `.toml` files)
- Output: none (mass_generator — produces output in `book` directory)

## Configuration

```toml
[processor.mdbook]
mdbook = "mdbook"                      # The mdbook command to run
output_dir = "book"                    # Output directory for generated docs
args = []                              # Additional arguments to pass to mdbook
extra_inputs = []                      # Additional files that trigger rebuilds when changed
cache_output_dir = true                # Cache the output directory for fast restore after clean
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `mdbook` | string | `"mdbook"` | The mdbook executable to run |
| `output_dir` | string | `"book"` | Output directory for generated documentation |
| `args` | string[] | `[]` | Extra arguments passed to mdbook |
| `extra_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |
| `cache_output_dir` | boolean | `true` | Cache the `book/` directory so `rsconstruct clean && rsconstruct build` restores from cache |
