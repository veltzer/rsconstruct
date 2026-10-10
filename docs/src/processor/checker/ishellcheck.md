# Ishellcheck Processor

## Purpose

Lints shell scripts with [ShellCheck](https://www.shellcheck.net/)'s analysis, in-process (no `shellcheck` program, no Haskell runtime to install). The Rust alternative to the [shellcheck](shellcheck.md) processor: a port of ShellCheck 0.11.0, so a script gets the same findings with the same codes, positions and messages, and passes or fails the way `shellcheck` would exit.

## How It Works

Each file goes through what ShellCheck does to it: its parser (sh, bash, dash, ksh and busybox dialects, here-documents, arithmetic, `[[ ]]` conditions, the parse errors and their notes), the control flow graph and its dataflow analysis (which assignments reach which uses, through functions and subshells), and every check of ShellCheck's `Analytics`, `Commands` and `ShellSupport` modules, optional checks included. The dialect comes from `shell`, a `# shellcheck shell=` directive, the shebang or the file extension, in ShellCheck's order. `# shellcheck` directives (`disable`, `enable`, `source`, `source-path`, `external-sources`, `extended-analysis`) and `.shellcheckrc` files (looked up from the script's directory upwards, then in `~/.shellcheckrc` and the XDG config directory) apply as in ShellCheck. Findings are printed as `shellcheck --format=gcc` prints them, and any finding fails the product:

```text
src/bad.sh:2:1: warning: foo appears unused. Verify use (or export if used externally). [SC2034]
src/bad.sh:3:6: note: Double quote to prevent globbing and word splitting. [SC2086]
```

Each file is its own product, checked as `shellcheck FILE` checks it: `source`d files are read only with `external_sources`, and their findings are reported only with `check_sourced`.

```toml
[processor.checker.ishellcheck]
src_dirs = ["scripts"]
exclude = ["SC1091"]
severity = "warning"
```

### Fidelity

Checked against shellcheck 0.11.0 over 4494 real scripts (1850 from the fleet, 2644 from a development machine's `/usr/bin` and friends), the 2194 snippets of ShellCheck's own unit tests, and 28000 mutated copies (broken quotes, brackets and keywords, swapped operators, deleted and duplicated lines): the same findings, line for line (about 360,000 compared), under default options, each `shell` override, `external_sources` and `check_sourced`. Neither side crashes on any of them.

It is also faster and smaller than shellcheck: 2781 real scripts (the fleet's and a development machine's) take 2.3 seconds against shellcheck's 15.7 on 16 threads (18 against 137 seconds of CPU), and the largest script, a 3500-line `dkms`, takes 1.0 second and 400 MB against 6.4 seconds and 1.9 GB.

The engine's unit tests (`src/engines/shellcheck/tests.rs`) keep it that way: they run `shellcheck` 0.11.0 itself (a missing or different version fails them) and the engine over the scripts in `src/engines/shellcheck/testdata/` and deterministic mutants of them, under each option, and require the same output line for line. The processor's tests check that each TOML field matches the shellcheck option of the same name.

Not supported: output formats other than `gcc`, auto-fixes (`--format=diff`), and the `--wiki-link-count`/`--color` presentation options.

## Source Files

- Input: `.sh` and `.bash` files under `src_dirs`, or the files in `src_files`
- Output: none (checker)

The default `src_dirs` is empty, so a stanza with no `src_dirs` or `src_files` checks nothing: name the directories (`[""]` for the whole project) or files to check.

## Configuration

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `shell` | string | `""` | The dialect, as `--shell` (`sh`, `bash`, `dash`, `ksh`, `busybox`); empty: from directives, the shebang or the extension |
| `external_sources` | bool | `false` | Follow `source`d files that are not inputs, as `--external-sources` |
| `source_path` | string[] | `[]` | Where to look for `source`d files, as `--source-path` (`SCRIPTDIR` is the script's directory) |
| `severity` | string | `"style"` | The minimum severity reported, as `--severity`: `error`, `warning`, `info`, `style` |
| `exclude` | string[] | `[]` | Codes to exclude, as `--exclude` (`SC2034` or `2034`) |
| `include` | string[] | `[]` | Codes to report exclusively, as `--include`; empty: all codes |
| `enable` | string[] | `[]` | Optional checks to enable, as `--enable` (`all` for every one) |
| `rcfile` | string | `""` | A `.shellcheckrc` to use instead of searching for one, as `--rcfile` |
| `norc` | bool | `false` | Ignore `.shellcheckrc` files, as `--norc` |
| `check_sourced` | bool | `false` | Report findings in `source`d files too, as `--check-sourced` |
| `extended_analysis` | string | `""` | Dataflow analysis, as `--extended-analysis`: `true`, `false`, or empty for the default (directives decide) |
| `dep_auto` | string[] | `[".shellcheckrc"]` | Config files that are inputs when they exist |

Switching a repo from shellcheck: `[processor.checker.shellcheck]` → `[processor.checker.ishellcheck]`; a `.shellcheckrc` stays as it is, and options in `args` become the fields of the same name (`args = ["-e", "SC1091", "-S", "warning"]` becomes `exclude = ["SC1091"]`, `severity = "warning"`).

## Batch support

Not used: checking in-process has no start-up cost for a batch to share, so files are checked in parallel as separate products.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
