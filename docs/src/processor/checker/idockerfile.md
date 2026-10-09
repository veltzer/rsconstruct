# Idockerfile Processor

## Purpose

Lints Dockerfiles with [hadolint](https://github.com/hadolint/hadolint)'s rules, in-process (no `hadolint` binary). The Rust alternative to the [hadolint](hadolint.md) processor: a port of hadolint 2.15.1's Dockerfile parser, its `# hadolint ignore=` pragmas, its `.hadolint.yaml` configuration and all of its `DL` rules, so a Dockerfile gets the same findings, with the same messages, codes and severities, and passes or fails the way `hadolint` would exit.

What is not here is ShellCheck. hadolint runs ShellCheck over every `RUN` script and reports its `SC` codes alongside its own; ShellCheck is a separate program with no Rust counterpart, so idockerfile reports `DL` findings only. A repo that wants the `SC` findings keeps the hadolint processor.

## How It Works

Each Dockerfile is parsed the way hadolint's parser (language-docker) reads it: instructions and their flags, escaped line breaks, heredocs, the `# escape=` and `# syntax=` pragmas, exec-form JSON arrays. `RUN` scripts are read as hadolint reads them for its rules: which commands run, with which arguments and flags, including commands inside `$(...)`, `if`, loops and subshells, and whether anything is piped. The 71 `DL` rules then run over the result, from `DL3000` (absolute `WORKDIR`) through the package-manager rules (`apt-get`, `apk`, `pip`, `npm`, `gem`, `yum`, `dnf`, `zypper`, `go`, `yarn`) to `DL4006` (`pipefail`). Findings are printed in hadolint's own format:

```text
Dockerfile:1 DL3006 warning: Always tag the version of an image explicitly
Dockerfile:4 DL3008 warning: Pin versions in apt get install. Instead of `apt-get install <package>` use `apt-get install <package>=<version>`
Dockerfile:4 DL3015 info: Avoid additional packages by specifying `--no-install-recommends`
Dockerfile:7 DL3059 info: Multiple consecutive `RUN` instructions. Consider consolidation.
```

A file fails when any finding is at least as severe as the failure threshold (`info` by default, like hadolint: `error` and `warning` and `info` fail, `style` does not). A file whose only findings are below the threshold passes, and nothing is printed for it. A Dockerfile hadolint's parser cannot read fails with the position of the problem.

### Configuration

idockerfile reads `.hadolint.yaml` (or `.hadolint.yml`) from the project root when one exists, exactly as `hadolint` run from there would, with the keys hadolint's checks use: `ignored`, `override` (`error`, `warning`, `info`, `style` lists), `trustedRegistries`, `label-schema`, `strict-labels`, `disable-ignore-pragma`, `failure-threshold`, `no-fail`. `config_file` names a different file. The TOML fields below add to or override what the file says. Inline pragmas work as in hadolint: `# hadolint ignore=DL3008,DL3015` for the next instruction, `# hadolint stage ignore=...` for the rest of the stage, `# hadolint global ignore=...` for the file.

```toml
[processor.checker.idockerfile]
src_files = ["Dockerfile", "docker/api/Dockerfile"]
ignored = ["DL3059"]
failure_threshold = "warning"
```

### Fidelity

Checked against hadolint 2.15.1 over the fleet's 154 Dockerfiles, with and without the fleet's `.hadolint.yaml`, and over 2000 mutated copies (unpinned installs, missing `-y`, `sudo`, `cd`, pipes, untagged and `latest` images, bad `COPY --from` references, duplicate stage names, `ONBUILD` wrappers, pragmas, heredocs, lower-cased keywords, exec-form commands): the same `DL` findings, file for file. Two things had to be copied from hadolint rather than reasoned out: its tree walk yields every shell command twice (once for ShellCheck's redirection wrapper, once for the command), which is what makes `DL3059` fire only for single-command `RUN`s; and a `RUN` script ShellCheck cannot parse (an unterminated quote, a redirection without a target) contributes no commands at all. One known divergence: hadolint 2.15.1 rejects a heredoc followed directly by the next instruction (a bug in its parser release, fixed upstream since); idockerfile accepts it.

## Source Files

- Input: files named `Dockerfile` under `src_dirs`, or the files in `src_files`
- Output: none (checker)

The default `src_dirs` is empty, so a stanza with no `src_dirs` or `src_files` checks nothing: name the directories (`[""]` for the whole project) or files to check.

## Configuration

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `config_file` | string | `""` | A hadolint configuration file (YAML). Empty: `.hadolint.yaml` or `.hadolint.yml` in the project root when present |
| `ignored` | string[] | `[]` | Rule codes to ignore, added to the configuration file's `ignored` |
| `trusted_registries` | string[] | `[]` | Registries `FROM` images may come from (DL3026); `*` wildcards allowed |
| `label_schema` | table | `{}` | Labels that must be present, `name = type` (`text`, `url`, `semver`, `hash`, `rfc3339`, `spdx`, `email`) |
| `strict_labels` | bool | `false` | Forbid labels outside `label_schema` (DL3050) |
| `disable_ignore_pragma` | bool | `false` | Ignore `# hadolint ignore=...` pragmas in the files |
| `failure_threshold` | string | `""` | Fail only on findings at least this severe: `error`, `warning`, `info`, `style`, `none`. Empty: the configuration file's, else `info` |
| `src_dirs` | string[] | `[]` | Directories to scan |
| `src_extensions` | string[] | `["Dockerfile"]` | File name suffixes to check |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |
| `dep_auto` | string[] | `[".hadolint.yaml", ".hadolint.yml"]` | Config files that are inputs when they exist |

Switching a repo from hadolint: `[processor.checker.hadolint]` → `[processor.checker.idockerfile]`; the `.hadolint.yaml` stays as it is. A repo whose findings include `SC` codes it wants to keep stays on hadolint.

## Batch support

Files are checked individually within a batch, so one bad file fails only its own product.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
