# Selene Processor

## Purpose

Lints Lua files using [selene](https://kampfkarren.github.io/selene/), a Lua
linter written in Rust. It is the Rust alternative to the
[luacheck](luacheck.md) processor: a single binary that needs no Lua runtime
and no LuaRocks.

## How It Works

Discovers `.lua` files under `src_dirs`, runs `selene` on them, and records
success in the cache. selene exits non-zero on warnings as well as errors,
so any finding fails the product, as with luacheck.

This processor supports batch mode, allowing multiple files to be checked in a
single selene invocation for better performance.

If `selene.toml` exists in the project root, it is automatically added as an
extra input so that configuration changes trigger rebuilds.

## Migrating from luacheck

selene is a different linter, not a port of luacheck, so a repo moving over
writes a `selene.toml` and expects different findings:

- **Lua version.** selene parses Lua 5.1 (and Luau). Its 0.32.0 release
  binary has no Lua 5.4 standard library at all, and `std = "lua52"` or
  `"lua53"` prints "feature for it is not enabled" and still parses 5.1, so
  `//`, bitwise operators, `goto` and `<const>` are parse errors. A source
  build with selene-lib's `lua53`/`lua54` features enabled behaves the same.
  Code written for Lua 5.3/5.4 stays on luacheck.
- **Host globals.** A global the program gets from its host (`vim`,
  `pandoc`) is declared in a custom standard library file next to
  `selene.toml` (`std = "lua51+neovim"` reads `neovim.yml`, which starts
  `base: lua51` and lists `globals:`), not on the command line as with
  luacheck's `--globals`.
- **Globals.** Assigning a global at the top level is a warning
  (`unscoped_variables`), and a global nothing in the same file reads is
  `unused_variable`. Globals one file defines and another reads through
  `dofile` are `undefined_variable` errors in the reading file: selene checks
  each file on its own and does not follow `dofile`. There is no counterpart
  of luacheck's `allow_defined_top`.
- **Scope.** One `selene.toml` covers the whole project; luacheck's
  per-directory `files["dir"]` overrides have no equivalent. Narrower
  exceptions are comments in the source: `-- selene: allow(lint_name)` for
  the next statement, `--# selene: allow(lint_name)` for the whole file.

## Source Files

- Input: `**/*.lua`
- Output: none (checker)

The default `src_dirs` is empty, so a stanza with no `src_dirs` or `src_files`
checks nothing: name the directories (`[""]` for the whole project) or files
to check.

## Configuration

```toml
[processor.checker.selene]
src_dirs = ["config"]
args = []
dep_inputs = []
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `command` | string | `"selene"` | The selene executable to run |
| `args` | string[] | `[]` | Extra arguments passed to selene |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds (a custom standard library file, say) |
| `dep_auto` | string[] | `["selene.toml"]` | Config files auto-detected as inputs |

## Installation

`rsconstruct tool install` downloads the Linux binary from the selene GitHub
release (a zip, extracted with `unzip`). The crate is the fallback.

## Batch support

The tool accepts multiple files on the command line. When batching is enabled (default), rsconstruct passes all files in a single invocation for better performance.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
