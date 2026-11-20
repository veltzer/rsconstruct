# Zizmor Processor

## Purpose

Audits GitHub Actions workflow files for security problems using
[zizmor](https://github.com/zizmorcore/zizmor), a Rust tool.

It covers the security half of what [actionlint](actionlint.md) and
[iactionlint](iactionlint.md) do, in more depth: actions not pinned to a
commit hash, credentials persisted by `actions/checkout`, `${{ }}` template
injection into `run:` scripts, excessive `permissions`, dangerous triggers
such as `pull_request_target`. It does not check workflow syntax or
expression types, so it is an addition to those processors, not a
replacement for them.

## How It Works

Discovers `.yml`/`.yaml` files in the configured directories, runs `zizmor`
on them, and records success in the cache. zizmor exits non-zero when it
reports a finding, which fails the product.

The default `args` are `--offline --no-progress`. Offline, zizmor runs every
audit that needs only the file; the audits that query GitHub (whether a tag
an action is pinned to still points at the same commit, known-vulnerable
action versions) are skipped. To run them, set `args = ["--no-progress"]`
and `sandbox_network = true`, and give zizmor a token in `GH_TOKEN`.

zizmor reads its own configuration from `.github/zizmor.yml` or `zizmor.yml`
(either may end in `.yaml`): ignored findings and per-audit policies. All
four paths are tracked via `dep_auto`, so editing the config reruns the
audit. A finding can also be silenced on its line with a
`# zizmor: ignore[<audit>]` comment.

## Source Files

- Input: `**/*.yml`, `**/*.yaml` under `src_dirs` (typically `.github/workflows`)
- Output: none (checker — a passing check is recorded in the cache, no file is written)

## Configuration

```toml
[processor.checker.zizmor]
src_dirs = [".github/workflows"]
args = ["--offline", "--no-progress"]        # The default
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `command` | string | `"zizmor"` | The zizmor executable to run |
| `args` | string[] | `["--offline", "--no-progress"]` | Arguments passed to zizmor before the files |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |
| `dep_auto` | string[] | (the four config paths above) | Config files that are inputs when they exist |

## Batch support

The tool accepts multiple files on the command line. When batching is enabled (default), rsconstruct passes all files in a single invocation for better performance.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
