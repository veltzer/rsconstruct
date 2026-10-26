# Aspell Processor

## Purpose

Checks spelling in Markdown files using [aspell](http://aspell.net/).

## How It Works

Discovers `.md` files in the project and runs `aspell` on each file using the
configured aspell configuration file. A non-zero exit code fails the product.

### Configuration files

`conf` is the aspell configuration, passed as `--conf`. The default
`.aspell.conf` is a `dep_auto` input, tracked when it exists and allowed to be
absent (aspell then runs on its built-in defaults). A `conf` other than the
default must exist — a missing one fails the build naming the field and the
file — and is an input of every product, so an edit to it re-checks every
file with no `dep_inputs` entry needed. The personal word list (`words_file`)
is an input of every product in the same way, so removing a word invalidates
the checks that relied on it.

## Source Files

- Input: `**/*.md`
- Output: none (checker)

## Configuration

```toml
[processor.checker.aspell]
command = "aspell"                     # The aspell command to run
conf = ".aspell.conf"                  # Aspell configuration file
args = []                              # Additional arguments to pass to aspell
dep_inputs = []                      # Additional files that trigger rebuilds when changed
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `command` | string | `"aspell"` | The aspell executable to run |
| `conf` | string | `".aspell.conf"` | Aspell configuration file, as `--conf`; a non-default one must exist and is an input of every product |
| `words_file` | string | `".aspell.en.pws"` | The personal word list; an input of every product |
| `auto_add_words` | bool | `false` | Add misspelled words to `words_file` instead of failing |
| `args` | string[] | `[]` | Extra arguments passed to aspell |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |
| `dep_auto` | string[] | `[".aspell.conf", ".aspell.en.pws", ".aspell.en.prepl"]` | Config files that are inputs when they exist |

## Batch support

The tool accepts multiple files on the command line. When batching is enabled (default), rsconstruct passes all files in a single invocation for better performance.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
