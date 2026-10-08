# Iyamllint Processor

## Purpose

Lints YAML files with [yamllint](https://github.com/adrienverge/yamllint)'s rules. Native (in-process; no Python, no external tools). The Rust alternative to the [yamllint](yamllint.md) processor: it reads the same `.yamllint.yaml`, runs the same 23 rules with the same options, and reports the same problems at the same positions.

## How It Works

Each file is tokenized (with libyaml, the scanner design yamllint's PyYAML implements too), then every enabled rule runs over its tokens, comments and lines, exactly as in yamllint's `linter.py`. Problems are reported in yamllint's parsable format, one per line:

```text
test.yaml:2:11: [error] trailing spaces (trailing-spaces)
test.yaml:4:7: [error] wrong indentation: expected 4 but found 6 (indentation)
test.yaml:1:1: [warning] missing document start "---" (document-start)
```

A problem at the `error` level fails the file. A `warning` is printed and the file passes, as a plain `yamllint` run passes; `strict = true` makes warnings fail too, like `yamllint --strict`.

### Configuration file

The yamllint configuration is found the way yamllint finds it: `.yamllint`, `.yamllint.yaml` or `.yamllint.yml` in the project root, in that order, else yamllint's built-in `default` configuration. `config_file` names a file explicitly instead. The user-level `~/.config/yamllint/config` that yamllint also consults is **not** read: a build must not depend on the machine it runs on.

Everything in yamllint's configuration format is supported, with two exceptions:

- `extends` (`default`, `relaxed`, or a path relative to the project root), `rules` with `enable`/`disable`/settings, `level`, every rule option, `ignore` and `ignore-from-file` at the top level and per rule: all honoured, with yamllint's own validation errors for an unknown rule, an unknown option or a wrong value. An invalid config fails the build.
- `yaml-files` is accepted and ignored: rsconstruct decides which files the processor scans, through `src_dirs`, `src_files` and `src_extensions`.
- `locale` is rejected. It changes how `key-ordering` collates; this processor compares code points, which is what yamllint does when no locale is set.

### Rules

All 23 rules: `anchors`, `braces`, `brackets`, `colons`, `commas`, `comments`, `comments-indentation`, `document-end`, `document-start`, `empty-lines`, `empty-values`, `float-values`, `hyphens`, `indentation`, `key-duplicates`, `key-ordering`, `line-length`, `new-line-at-end-of-file`, `new-lines`, `octal-values`, `quoted-strings`, `trailing-spaces`, `truthy`. See [yamllint's rule documentation](https://yamllint.readthedocs.io/en/stable/rules.html) for their options.

The `# yamllint disable`, `# yamllint enable`, `# yamllint disable-line` and `# yamllint disable-file` directives work as in yamllint.

Syntax errors are reported like yamllint's, at the position the parser gives up. The wording differs: it is libyaml's (`did not find expected key`) rather than PyYAML's (`expected <block end>, but found '?'`). One input the two parsers disagree on: inside a flow collection, a colon directly followed by `,` `]` `}` `?` `[` or `{` (`{a: 1, b:, c: 3}`) is a syntax error for libyaml (`found unexpected ':'`), while PyYAML reads an empty value. Write `b: ,` or `b: null`.

### Fidelity

The port follows yamllint's source file by file (`src/yamllint/` mirrors `yamllint/parser.py`, `config.py`, `linter.py` and `rules/`). It was checked against yamllint 1.38.0 over the fleet's 362 tracked YAML files, under each repository's own `.yamllint.yaml` and again under yamllint's stricter `default` configuration: every one of the 1505 problems yamllint reported was reported at the same file, line, column, level and rule, and nothing else was.

## Source Files

- Input: `.yml` and `.yaml` files under `src_dirs`
- Output: none (checker)

The default `src_dirs` is empty, so a stanza with no `src_dirs` or `src_files` checks nothing: name the directories (`[""]` for the whole project) or files to check.

The config files (`.yamllint`, `.yamllint.yaml`, `.yamllint.yml`) are `dep_auto` inputs: a change to one re-checks every file. A file named by `config_file` is not tracked automatically; add it to `dep_inputs` if it lives elsewhere.

## Configuration

```toml
[processor.checker.iyamllint]
src_dirs = [".github"]
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `config_file` | string | `""` | yamllint config file to read; empty means `.yamllint`, `.yamllint.yaml` or `.yamllint.yml` in the project root, else yamllint's `default` configuration |
| `strict` | bool | `false` | Fail on warnings too, like `yamllint --strict` |
| `src_dirs` | string[] | `[]` | Directories to scan |
| `src_extensions` | string[] | `[".yml", ".yaml"]` | File extensions to check |
| `dep_auto` | string[] | `[".yamllint", ".yamllint.yml", ".yamllint.yaml"]` | Config files whose changes re-check every file |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

Files are checked individually within a batch, so one bad file fails only its own product.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
