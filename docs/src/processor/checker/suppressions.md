# Suppressions Processor

## Purpose

Checks that every inline lint suppression names what it suppresses and says why. Builtin (in-process; no external tool).

A linter's inline comment (`# noqa`, `// cppcheck-suppress`, `# shellcheck disable=`, ...) is the one place where a finding is switched off without anyone seeing it again. This checker keeps that place honest: a suppression that names no rule, covers a whole file, or carries no reason fails the build.

## How It Works

Each file is read in the comment syntax of its language, so a directive of another language inside a string is not taken for one. On every directive it finds, three things are checked:

- **It names a rule.** `# noqa`, `# type: ignore`, `// NOLINT`, `## no critic` and `// cppcheck-suppress` with nothing after them are refused; `# noqa: E731`, `# type: ignore[arg-type]`, `// NOLINT(bugprone-foo)`, `## no critic (Policy::Name)` are the accepted forms.
- **It covers one line.** The forms that switch a linter off for a whole file or the rest of it are refused: `# ruff: noqa`, `# flake8: noqa`, `# pyrefly: ignore-errors`, `eslint-disable` and `oxlint-disable` without `-line` or `-next-line`, `yamllint disable` without `-line`, `rumdl-disable` and `markdownlint-disable` without `-line` or `-next-line`, `stylelint-disable` likewise. In Rust, `#[allow(...)]` is refused in favour of `#[expect(...)]`, which rustc itself fails once it suppresses nothing.
- **It gives a reason.** Either text after the directive on its own line, or a comment on the line directly above it. Several directives stacked on consecutive lines share the comment above the first. A Rust `#[expect(...)]` carries its reason as the `reason = "..."` argument.

```text
src/a.py:12: `noqa` names no rule
src/b.c:40: `cppcheck-suppress` gives no reason: say why on the same line or in a comment directly above
web/c.js:1: `eslint-disable` covers the rest of the file: suppress the one line
src/d.rs:7: `#[allow]` use #[expect(..., reason = "...")], which also fails once it suppresses nothing
```

The checker does not judge what the reason says, and it does not know whether the suppression still suppresses anything. The linters' own rules do the second: ruff's `RUF100`, rumdl's `MD087`, cppcheck's `unmatchedSuppression`, rustc's `expect`.

### Directives by language

| Files | Directives read |
|-------|-----------------|
| `.py` | `# noqa`, `# type: ignore`, `# pyrefly: ignore`, `# pylint: disable=`, and the file-wide `# ruff: noqa`, `# flake8: noqa`, `# pyrefly: ignore-errors` |
| `.sh`, `.bash` | `# shellcheck disable=` |
| `.c`, `.cc`, `.cpp`, `.cxx`, `.h`, `.hh`, `.hpp` | `cppcheck-suppress`, `NOLINT`, `NOLINTNEXTLINE`, `NOLINTBEGIN` |
| `.java` | `CHECKSTYLE:OFF` |
| `.pl`, `.pm` | `## no critic` |
| `.js`, `.mjs`, `.cjs`, `.ts` | `eslint-disable*`, `oxlint-disable*`, `biome-ignore` |
| `.css` | `biome-ignore`, `stylelint-disable*` |
| `.yml`, `.yaml` | `yamllint disable*`, `zizmor: ignore[...]` |
| `Dockerfile`, `Dockerfile.*` | `hadolint ignore=`, `shellcheck disable=` |
| `.md` | `rumdl-disable*`, `markdownlint-disable*` |
| `.lua` | `luacheck: ignore` |
| `.rs` | `#[allow(...)]`, `#[expect(...)]`, and their `#![...]` and `cfg_attr` forms |

## Source Files

- Input: the files under `src_dirs` and in `src_files` whose extension is in the table above
- Output: none (checker)

The default `src_dirs` is empty, so a stanza with no `src_dirs` or `src_files` checks nothing. Name the directories of the code the project wrote; a vendored library or a generated file carries the suppressions of whoever wrote it and is left out by not being named.

## Configuration

```toml
[processor.checker.suppressions]
src_dirs = ["src", "scripts", ".github"]
src_files = ["Dockerfile"]
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `src_dirs` | string[] | `[]` | Directories to scan |
| `src_files` | string[] | `[]` | Single files to check |
| `src_extensions` | string[] | every extension in the table above | File extensions to check |
| `src_exclude_dirs` | string[] | `[]` | Directories under `src_dirs` to leave out |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

Files are checked individually within a batch, so one bad file fails only its own product.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
