# Luacheck Processor

## Purpose

Lints Lua scripts using [luacheck](https://github.com/lunarmodules/luacheck).

## How It Works

Discovers `.lua` files in the project (excluding common build tool
directories), runs `luacheck` on each file, and records success in the cache.
A non-zero exit code from luacheck fails the product.

This processor supports batch mode, allowing multiple files to be checked in a
single luacheck invocation for better performance.

## Source Files

- Input: `**/*.lua`
- Output: none (linter)

## Configuration

```toml
[processor.luacheck]
command = "luacheck"                         # The luacheck command to run
args = []                                    # Additional arguments to pass to luacheck
dep_inputs = []                            # Additional files that trigger rebuilds when changed
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `command` | string | `"luacheck"` | The luacheck executable to run |
| `args` | string[] | `[]` | Extra arguments passed to luacheck |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

The tool accepts multiple files on the command line. When batching is enabled (default), rsconstruct passes all files in a single invocation for better performance.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../processors.md#clean-behavior) and [`rsconstruct clean`](../commands.md#rsconstruct-clean).
