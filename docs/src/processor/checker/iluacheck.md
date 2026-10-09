# Iluacheck Processor

## Purpose

Lints Lua files with [luacheck](https://github.com/lunarmodules/luacheck)'s analysis, in-process (no `luacheck` program, no Lua runtime to install). The Rust alternative to the [luacheck](luacheck.md) processor: a port of luacheck 1.2.0, so a set of files gets the same findings with the same codes and messages, and passes or fails the way `luacheck` would exit.

## How It Works

Each file goes through what luacheck does to it: its lexer and parser (Lua 5.1 through 5.4 and LuaJIT syntax: `goto` and labels, integer division and bitwise operators, `<const>`/`<close>` attributes, compound assignments, cdata literals), the linearized flow graph of every function, the resolution of which assignments reach which accesses (closures included), and every check luacheck has: globals and their fields traced through `local t = table`, unused and overwritten variables, values and arguments, shadowing, unreachable code, empty blocks, unbalanced assignments, duplicate table keys, cyclomatic complexity, whitespace and line length. `-- luacheck:` inline comments (`ignore`, `globals`, `push`/`pop`, ...) apply as in luacheck, including their errors. Findings are printed as `luacheck --formatter plain --codes` prints them, and any finding fails the build:

```text
config/project.lua:1:7: (W211) unused variable 'x'
config/project.lua:2:1: (W111) setting non-standard global variable 'Y'
config/project.lua:4:1: (E011) expected 'end' (to close 'if' on line 1) near 'local' (indentation-based guess)
```

All the files of a stanza are checked together, as one `luacheck` run checks the files given to it: with `allow_defined` or `allow_defined_top`, a global set in one file is defined for all the others, and an implicitly defined global no file reads is reported as unused (`W131`). The processor therefore makes a single product of all its files, rechecked as a whole when any of them changes.

### Configuration

`.luacheckrc` is found and run as luacheck runs it: looked up from the project root upwards, executed as Lua (in the embedded interpreter) in an environment that falls back to the standard globals, with `files["glob"]` per-path overrides, custom `stds`, and a returned table merged in. luacheck's default per-path stds apply (`busted` for `*_spec.lua` under `spec/`, `test/` or `tests/`, `rockspec`, `luacheckrc`, `ldoc`). `exclude_files` and `include_files` filter the checked files as in luacheck. `config_file` names a different file; empty means no configuration file (`--no-config`).

The TOML fields are luacheck's command-line options and override the configuration file the same way:

```toml
[processor.checker.iluacheck]
src_dirs = ["config"]
std = "lua54"
globals = ["vim"]
ignore = ["212/self"]
max_line_length = 100
```

### Fidelity

Checked against luacheck 1.2.0 (the apt package, which runs on Lua 5.1) over the 7101 Lua files on a development machine (Neovim plugins, nmap's NSE libraries, TeX Live, lua-language-server, ardour scripts), luacheck's own sources and samples, the fleet's Lua files, and 3000 mutated copies (broken tokens and escapes, missing `end`s, labels and `goto`, varargs, attributes, compound operators, duplicate keys, inline options valid and invalid), under default options, ten command-line option sets and three `.luacheckrc` configurations: the same findings, line for line (about 2.4 million compared). Configuration errors give luacheck's messages. One difference is inherent: a runtime error raised inside `.luacheckrc` is worded by the interpreter running it, Lua 5.4 here and Lua 5.1 in luacheck (`attempt to index a nil value (local 't')` instead of `attempt to index local 't' (a nil value)`).

Not supported: the global configuration file (`~/.config/luacheck/.luacheckrc`), checking rockspecs as file lists, the cache, and luacheck's output formatters other than `plain` with codes.

## Source Files

- Input: `.lua` files under `src_dirs`, or the files in `src_files`
- Output: none (checker)

The default `src_dirs` is empty, so a stanza with no `src_dirs` or `src_files` checks nothing: name the directories (`[""]` for the whole project) or files to check.

## Configuration

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `config_file` | string | `".luacheckrc"` | The configuration file, as `--config`; empty for `--no-config` |
| `std` | string | `""` | Standard globals, as `--std` (`max`, `lua51`, `lua54+busted`, ...); empty: unset |
| `globals` | string[] | unset | Custom globals or fields, as `--globals` |
| `read_globals` | string[] | unset | Read-only globals or fields, as `--read-globals` |
| `new_globals` | string[] | unset | Globals replacing those set before, as `--new-globals` |
| `new_read_globals` | string[] | unset | Read-only globals replacing those set before, as `--new-read-globals` |
| `not_globals` | string[] | unset | Globals or fields to remove, as `--not-globals` |
| `ignore` | string[] | unset | Patterns of warnings to filter out (code, name, or `code/name`), as `--ignore` |
| `enable` | string[] | unset | Patterns of warnings not to filter out, as `--enable` |
| `only` | string[] | unset | Report only warnings matching these patterns, as `--only` |
| `operators` | string[] | unset | Compound operators to allow, as `--operators` |
| `allow_defined` | bool | `false` | `--allow-defined` |
| `allow_defined_top` | bool | `false` | `--allow-defined-top` |
| `module` | bool | `false` | `--module` |
| `no_global`, `no_unused`, `no_redefined`, `no_unused_args`, `no_unused_secondaries`, `no_self` | bool | `false` | `-g`, `-u`, `-r`, `-a`, `-s`, `--no-self` |
| `max_line_length`, `max_code_line_length`, `max_string_line_length`, `max_comment_line_length` | integer | unset | The `--max-...-line-length` limits; 0 for `--no-max-...-line-length` |
| `max_cyclomatic_complexity` | integer | unset | `--max-cyclomatic-complexity`; 0 for no limit |
| `dep_auto` | string[] | `[".luacheckrc"]` | Config files that are inputs when they exist |

Switching a repo from luacheck: `[processor.checker.luacheck]` → `[processor.checker.iluacheck]`; the `.luacheckrc` stays as it is, and options in `args` become the fields of the same name (`args = ["--globals", "vim", "--"]` becomes `globals = ["vim"]`).

## Batch support

Not applicable: the processor's one product covers all its files.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
