# Luacheck Processor

## Purpose

Lints Lua scripts using [luacheck](https://github.com/lunarmodules/luacheck).
Rust alternatives are [iluacheck](iluacheck.md), a port of luacheck with the same findings, and [selene](selene.md).

## How It Works

Discovers `.lua` files in the project (excluding common build tool
directories), runs `luacheck` on each file, and records success in the cache.
A non-zero exit code from luacheck fails the product.

This processor supports batch mode, allowing multiple files to be checked in a
single luacheck invocation for better performance.

`.luacheckrc` in the project root is an input of every product when it exists
(`dep_auto`): an edit re-checks every file. Because `.luacheckrc` applies
options per path (`files["glob"]`) and luacheck picks a default std from the
path (`*_spec.lua` under `spec/`), a file's verdict depends on its name as
well as its content, so cached results are keyed by both
(`output_depends_on_input_name`), as for [iluacheck](iluacheck.md).

## Source Files

- Input: `**/*.lua`
- Output: none (linter)

## Configuration

```toml
[processor.checker.luacheck]
command = "luacheck"                         # The luacheck command to run
args = []                                    # Additional arguments to pass to luacheck
dep_inputs = []                            # Additional files that trigger rebuilds when changed
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `command` | string | `"luacheck"` | The luacheck executable to run |
| `args` | string[] | `[]` | Extra arguments passed to luacheck |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |
| `dep_auto` | string[] | `[".luacheckrc"]` | Config files that are inputs when they exist |
| `output_depends_on_input_name` | bool | `true` | Key cached results by file path as well as content, because `.luacheckrc` options and the default std follow the path. A renamed file is re-checked instead of reusing its old verdict. See [`output_depends_on_input_name`](../../configuration.md) |

## Batch support

The tool accepts multiple files on the command line. When batching is enabled (default), rsconstruct passes all files in a single invocation for better performance.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
