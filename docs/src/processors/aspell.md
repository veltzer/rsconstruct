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
command = "aspell"                     # The aspell command to run
conf = ".aspell.conf"                  # Aspell configuration file
args = []                              # Additional arguments to pass to aspell
dep_inputs = []                      # Additional files that trigger rebuilds when changed
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `command` | string | `"aspell"` | The aspell executable to run |
| `conf` | string | `".aspell.conf"` | Aspell configuration file |
| `args` | string[] | `[]` | Extra arguments passed to aspell |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

The tool accepts multiple files on the command line. When batching is enabled (default), rsconstruct passes all files in a single invocation for better performance.
