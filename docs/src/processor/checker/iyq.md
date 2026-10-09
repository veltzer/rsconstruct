# Iyq Processor

## Purpose

Checks YAML files with a jq filter, in-process (no `yq`, no `jq`). The Rust alternative to the [yq](yq.md) checker, which runs `yq .` over every YAML file: each file is read as YAML, every document in it is turned into JSON, and the filter is evaluated over each document with [jaq](https://github.com/01mf02/jaq), a Rust implementation of jq. A file that is not valid YAML, or on which the filter raises an error, fails.

## How It Works

With the default filter, `.`, a file passes when it parses as YAML: the check `yq .` performs. Any jq filter works, so a stanza can assert a shape:

```toml
[processor.checker.iyq]
src_dirs = ["deploy"]
filter = 'if has("apiVersion") and has("kind") then . else error("not a Kubernetes object") end'
```

The filter runs over every document of a multi-document file in turn; the first error names the file and the document:

```text
deploy/web.yaml: document 2: jq: "not a Kubernetes object"
```

A filter that does not parse is a configuration error naming the filter. The filter's outputs are not written anywhere: this is a checker, like the `yq` processor it replaces.

### Fidelity

Checked against `yq` (the jq wrapper, version 4) over the fleet's 1015 YAML files under the filters `.` and `[.. | strings] | length`: the same files pass and fail. Two details of `yq` had to be matched: a leading byte order mark is skipped (PyYAML does; libyaml does not), and each document is handed to the filter on its own.

## Source Files

- Input: `.yml` and `.yaml` files under `src_dirs`
- Output: none (checker)

The default `src_dirs` is empty, so a stanza with no `src_dirs` or `src_files` checks nothing: name the directories (`[""]` for the whole project) or files to check.

## Configuration

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `filter` | string | `"."` | The jq filter evaluated over every document; `.` only checks that the YAML parses |
| `src_dirs` | string[] | `[]` | Directories to scan |
| `src_extensions` | string[] | `[".yml", ".yaml"]` | File extensions to check |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

Switching a repo from yq: `[processor.checker.yq]` → `[processor.checker.iyq]`; `args = ["FILTER"]` → `filter = "FILTER"`.

## Batch support

Files are checked individually within a batch, so one bad file fails only its own product.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
