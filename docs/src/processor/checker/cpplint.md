# Cpplint Processor

## Purpose

Lints C/C++ files using [cpplint](https://github.com/cpplint/cpplint) (Google C++ style checker).

The Rust alternative is [icpplint](icpplint.md): cpplint 2.0.2's checks, `CPPLINT.cfg` chain and `NOLINT` handling, in-process, with cpplint's messages and exit rule. Switching is `cpplint` → `icpplint` in the stanza header, with `args = ["--config=NAME"]` becoming `config_file = "NAME"`.

## How It Works

Discovers `.c`, `.cc`, `.h`, and `.hh` files under `src/` (excluding common
C/C++ build directories), runs `cpplint` on each file, and records success in
the cache. A non-zero exit code from cpplint fails the product.

This processor supports batch mode.

cpplint reads `CPPLINT.cfg` from the file's directory and every parent, so the
project root's `CPPLINT.cfg` is an input of every product when it exists
(`dep_auto`): an edit to it re-checks every file, with no `dep_inputs` entry
needed. A `CPPLINT.cfg` in a subdirectory is not tracked; list it in
`dep_inputs`.

## Source Files

- Input: `src/**/*.c`, `src/**/*.cc`, `src/**/*.h`, `src/**/*.hh`
- Output: none (checker)

## Configuration

```toml
[processor.checker.cpplint]
args = []
dep_inputs = []
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `args` | string[] | `[]` | Extra arguments passed to cpplint |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |
| `dep_auto` | string[] | `["CPPLINT.cfg"]` | Config files that are inputs when they exist |

## Batch support

The tool accepts multiple files on the command line. When batching is enabled (default), rsconstruct passes all files in a single invocation for better performance.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
