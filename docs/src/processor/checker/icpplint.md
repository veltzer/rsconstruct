# Icpplint Processor

## Purpose

Lints C and C++ files with [cpplint](https://github.com/cpplint/cpplint)'s rules, in-process (no `cpplint` program, no Python). The Rust alternative to the [cpplint](cpplint.md) processor: a port of cpplint 2.0.2, so a file gets the same findings with the same messages, categories and confidence levels, and passes or fails the way `cpplint` would exit.

## How It Works

Each file goes through what cpplint does to it: the four line views (raw, raw strings blanked, comments removed, strings collapsed), the nesting stack of namespaces, classes, `extern "C"` blocks, constructors and preprocessor branches, the include-order and header-guard state, `NOLINT`/`NOLINTNEXTLINE`/`NOLINTBEGIN`/`NOLINTEND` suppressions, and every check cpplint has, from `legal/copyright` to `whitespace/todo` (the sixty-six categories `cpplint --filter=` lists). Findings are printed in cpplint's own format and any finding fails the file:

```text
src/main.cc:0:  No copyright message found.  You should have a line: "Copyright [year] <Copyright Owner>"  [legal/copyright] [5]
src/main.cc:1:  Missing space before {  [whitespace/braces] [5]
src/main.cc:2:  Tab found; better to use spaces  [whitespace/tab] [1]
src/main.cc:2:  Using C-style cast.  Use static_cast<int>(...) instead  [readability/casting] [4]
```

Header guards are derived the way cpplint derives them: from the path relative to the repository root (the nearest directory with `.git`, `.hg` or `.svn`, or `repository`), adjusted by `root`. The file name cpplint sees is the path relative to the project root, which is what `cpplint src/foo.cc` run from the project root would see, so filters with a file part (`-whitespace:src/foo.cc:14`) use that path.

### Configuration

cpplint's per-directory configuration files work as they do in cpplint: `CPPLINT.cfg` (or the file `config_file` names) is read from the file's directory upwards until one says `set noparent`, with `filter`, `exclude_files`, `linelength`, `root`, `headers`, `extensions` and `includeorder`. The TOML fields are cpplint's command-line options:

```toml
[processor.checker.icpplint]
src_dirs = ["src"]
config_file = ".cpplint"
filters = ["-whitespace/tab", "-build/include_order"]
line_length = 100
```

A file an `exclude_files` pattern matches is skipped, as cpplint skips it. A configuration file with an option cpplint does not know, a non-numeric `linelength`, a filter that does not start with `+` or `-`, or a file whose extension is not one cpplint accepts fails the build; cpplint prints a message and goes on, which hides the mistake.

### Fidelity

Checked against cpplint 2.0.2 over its own sample corpus (`samples/*/*.def`: seven projects under twelve option sets, including `CPPLINT.cfg`, `--filter`, `--includeorder=standardcfirst`, `--headers`, `--recursive --exclude`), the 1245 snippets its unit tests lint, demos-os-linux's 1231 C/C++ files with and without the repo's `.cpplint`, and 3000 mutated copies of those files (tabs, casts, missing spaces, `using namespace`, VLAs, `sprintf`, `NOLINT` variants, header-guard and include edits, CRLF line endings, no final newline): the same findings, file for file. Two things cpplint does are deliberately not reproduced: its "Unexpected \r (^M)" warning, which cpplint itself can never emit because Python's text mode translates line endings before it looks; and the way a `linelength`, `root`, `headers` or `extensions` setting read from one directory's `CPPLINT.cfg` stays in force for every file cpplint processes afterwards in the same run (here every file starts from the configured options).

## Source Files

- Input: `.c`, `.cc`, `.cpp`, `.cxx`, `.c++`, `.cu`, `.h`, `.hh`, `.hpp`, `.hxx`, `.h++`, `.cuh` files under `src_dirs`, or the files in `src_files`
- Output: none (checker)

The default `src_dirs` is empty, so a stanza with no `src_dirs` or `src_files` checks nothing: name the directories (`[""]` for the whole project) or files to check.

## Configuration

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `filters` | string[] | `[]` | Category filters as `--filter`: `-whitespace`, `+whitespace/braces`, `-runtime/printf:foo.cc:14` (an entry may hold several, comma-separated) |
| `verbose` | integer | `1` | Report only findings with at least this confidence (0-5), as `--verbose` |
| `line_length` | integer | `80` | The allowed line length, as `--linelength` |
| `root` | string | `""` | The directory header guards are derived from, as `--root`. Empty: unset |
| `repository` | string | `""` | The repository top directory, as `--repository`. Empty: the nearest `.git`, `.hg` or `.svn` |
| `headers` | string[] | `[]` | Extensions (without the dot) treated as headers, as `--headers` |
| `extensions` | string[] | `[]` | Extensions (without the dot) cpplint accepts, as `--extensions` |
| `include_order` | string | `"default"` | `default` or `standardcfirst`, as `--includeorder` |
| `config_file` | string | `"CPPLINT.cfg"` | The name of the per-directory configuration file, as `--config` |
| `src_dirs` | string[] | `[]` | Directories to scan |
| `src_extensions` | string[] | the twelve above | File name suffixes to check |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |
| `dep_auto` | string[] | `["CPPLINT.cfg"]` | Config files that are inputs when they exist |

Switching a repo from cpplint: `[processor.checker.cpplint]` → `[processor.checker.icpplint]`, and `args = ["--config=.cpplint"]` becomes `config_file = ".cpplint"` (keep `dep_inputs = [".cpplint"]`, since `dep_auto` only knows the default name). Any other `--option=value` in `args` becomes the field of the same name.

## Batch support

Files are checked individually within a batch, so one bad file fails only its own product.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
