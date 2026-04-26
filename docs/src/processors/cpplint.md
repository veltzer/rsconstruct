# Cpplint Processor

## Purpose

Lints C/C++ files using [cpplint](https://github.com/cpplint/cpplint) (Google C++ style checker).

## How It Works

Discovers `.c`, `.cc`, `.h`, and `.hh` files under `src/` (excluding common
C/C++ build directories), runs `cpplint` on each file, and records success in
the cache. A non-zero exit code from cpplint fails the product.

This processor supports batch mode.

## Source Files

- Input: `src/**/*.c`, `src/**/*.cc`, `src/**/*.h`, `src/**/*.hh`
- Output: none (checker)

## Configuration

```toml
[processor.cpplint]
args = []
dep_inputs = []
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `args` | string[] | `[]` | Extra arguments passed to cpplint |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

The tool processes one file at a time. Each file is checked in a separate invocation.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../processors.md#clean-behavior) and [`rsconstruct clean`](../commands.md#rsconstruct-clean).
