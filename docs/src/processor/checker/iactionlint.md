# Iactionlint Processor

## Purpose

Lints GitHub Actions workflow files with [actionlint](https://github.com/rhysd/actionlint)'s rules. Native (in-process; no Go binary, no network). The Rust alternative to the [actionlint](actionlint.md) processor: it reads the same `.github/actionlint.yaml`, runs the same checks, and reports the same problems at the same positions, in actionlint's own words.

## How It Works

Each workflow file is parsed into actionlint's syntax tree and every rule actionlint runs is applied: the structure of the file (`syntax-check`), every `${{ }}` expression parsed and type-checked with the contexts available where it sits (`expression`), `on:` events, activity types, filters and cron schedules (`events`, `glob`), `runs-on` labels (`runner-label`), `needs` graphs (`job-needs`), job and step IDs (`id`), `permissions`, `shell`, `env`, matrix combinations (`matrix`), credentials, deprecated workflow commands, constant `if:` conditions (`if-cond`), `uses:` of popular actions, local actions and reusable workflows (`action`, `workflow-call`). Problems are reported in actionlint's format, one per line:

```text
.github/workflows/ci.yml:4:14: label "ubuntu-lates" is unknown. available labels are ... [runner-label]
.github/workflows/ci.yml:6:25: property "foo" is not defined in object type {action: string; ...} [expression]
.github/workflows/ci.yml:3:3: "runs-on" section is missing in job "test" [syntax-check]
```

Any problem fails the file, as any problem makes `actionlint` exit 1.

### What is not ported

actionlint can run `shellcheck` over `run:` scripts and `pyflakes` over Python steps when those tools are installed. Those two rules call external programs and are not part of this processor. A repository that wants its workflow scripts shell-checked keeps the [actionlint](actionlint.md) processor, or checks the scripts another way.

### Configuration file

actionlint's configuration is found the way actionlint finds it: `.github/actionlint.yaml`, then `.github/actionlint.yml`, else none. `config_file` names a file explicitly instead. All of it is honoured: `self-hosted-runner.labels` (plain names or globs), `config-variables` (`null` disables the check, an empty list allows none), and `paths` with per-path `ignore` regular expressions matched against error messages.

### Popular actions

actionlint knows the inputs and outputs of a few hundred widely used actions (`actions/checkout@v4` and friends) and which old versions no longer run on GitHub's runners. The same data set is embedded here, generated from actionlint's `popular_actions.go` by `scripts/gen-actionlint-popular-actions.py`; re-run it when moving to a newer actionlint release. Local actions (`uses: ./path`) are read from `action.yml` in the project, as actionlint reads them; remote actions outside the data set are not fetched, by either tool.

### Fidelity

The port follows actionlint v1.7.12 file by file (`src/engines/actionlint/` mirrors `parse.go`, `expr_*.go` and `rule_*.go`). It was checked against actionlint's own test corpus: every one of the 740 problems actionlint reports over its `testdata/err`, `testdata/examples`, `testdata/ok` and `testdata/projects` cases (local actions, reusable workflows, configuration files included) is reported at the same position with the same message, and nothing else is. Over the fleet's 343 workflow files in 219 repositories both tools report nothing.

## Source Files

- Input: `.yml` and `.yaml` files under `src_dirs`
- Output: none (checker)

The default `src_dirs` is empty, so a stanza with no `src_dirs` or `src_files` checks nothing: name `.github/workflows`.

The config files (`.github/actionlint.yaml`, `.github/actionlint.yml`) are `dep_auto` inputs: a change to one re-checks every file. A file named by `config_file` is not tracked automatically; add it to `dep_inputs` if it lives elsewhere. Local actions and reusable workflows a workflow refers to are read when the workflow is checked; list them in `dep_inputs` to re-check on their changes.

## Configuration

```toml
[processor.checker.iactionlint]
src_dirs = [".github/workflows"]
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `config_file` | string | `""` | actionlint config file to read; empty means `.github/actionlint.yaml`, then `.github/actionlint.yml`, else no configuration |
| `src_dirs` | string[] | `[]` | Directories to scan |
| `src_extensions` | string[] | `[".yml", ".yaml"]` | File extensions to check |
| `dep_auto` | string[] | `[".github/actionlint.yaml", ".github/actionlint.yml"]` | Config files whose changes re-check every file |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

Files are checked individually within a batch, so one bad file fails only its own product.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
