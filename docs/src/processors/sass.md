# Sass Processor

## Purpose

Compiles SCSS and SASS files into CSS using the Sass compiler.

## How It Works

Files matching configured extensions in the `sass/` directory are compiled to CSS.
Output is written to `out/sass/` preserving the directory structure:

```
sass/style.scss  ->  out/sass/style.css
sass/components/button.scss  ->  out/sass/components/button.css
```

## Source Files

- Input: `sass/**/*{extensions}`
- Output: `out/sass/` mirroring the source structure with `.css` extension

## Configuration

```toml
[processor.sass]
sass_bin = "sass"                         # Sass compiler binary (default: "sass")
extensions = [".scss", ".sass"]           # File extensions to process
output_dir = "out/sass"                   # Output directory (default: "out/sass")
extra_inputs = []                         # Additional files that trigger rebuilds
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `sass_bin` | string | `"sass"` | Path to sass compiler |
| `extensions` | string[] | `[".scss", ".sass"]` | File extensions to discover |
| `output_dir` | string | `"out/sass"` | Output directory |
| `extra_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

Each input file is processed individually, producing its own output file.
