# Iyamllint Processor

## Purpose

Checks that YAML files parse. Native (in-process, using `serde_yaml_ng`; no external tools required). An in-process alternative to [yamllint](yamllint.md) for syntax only.

## How It Works

Each file is read and parsed as YAML. A file that does not parse fails its product with the parser's message and position:

```text
bad.yaml: [processor.checker.iyamllint] Invalid YAML:
bad.yaml: did not find expected ',' or ']' at line 2 column 1, while parsing a flow sequence at line 1 column 4
```

What it covers, compared to yamllint:

- **Syntax errors** are reported.
- **Duplicate mapping keys** are reported (`duplicate entry with key "a"`).
- **None of yamllint's style rules** run (line length, indentation, truthy values, document start, ...), and `.yamllint.yaml` is not read.
- **Multi-document files are accepted.** Each document separated by `---` is parsed in turn, so a file holding several documents is valid, as it is for yamllint. The first document that does not parse fails the file.

## Source Files

- Input: `.yml` and `.yaml` files under `src_dirs`
- Output: none (checker)

The default `src_dirs` is empty, so a stanza with no `src_dirs` or `src_files` checks nothing: name the directories (`[""]` for the whole project) or files to check.

## Configuration

```toml
[processor.checker.iyamllint]
src_dirs = [".github"]
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `src_dirs` | string[] | `[]` | Directories to scan |
| `src_extensions` | string[] | `[".yml", ".yaml"]` | File extensions to check |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

Files are checked individually within a batch, so one bad file fails only its own product.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
