# Aspell Processor

## Purpose

Checks spelling in Markdown files using [aspell](http://aspell.net/).

## How It Works

Discovers `.md` files in the project and runs `aspell` on each file using the
configured aspell configuration file. A non-zero exit code fails the product.

## Source Files

- Input: `**/*.md`
- Output: none (checker)

## Configuration

```toml
[processor.aspell]
aspell = "aspell"                      # The aspell command to run
conf_dir = "."                         # Configuration directory
conf = ".aspell.conf"                  # Aspell configuration file
args = []                              # Additional arguments to pass to aspell
extra_inputs = []                      # Additional files that trigger rebuilds when changed
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `aspell` | string | `"aspell"` | The aspell executable to run |
| `conf_dir` | string | `"."` | Directory containing the aspell configuration |
| `conf` | string | `".aspell.conf"` | Aspell configuration file |
| `args` | string[] | `[]` | Extra arguments passed to aspell |
| `extra_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |
