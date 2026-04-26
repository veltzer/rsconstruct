# Clang-Tidy Processor

## Purpose

Runs clang-tidy static analysis on C/C++ source files.

## How It Works

Discovers `.c` and `.cc` files under the configured source directory, runs
clang-tidy on each file individually, and creates a stub file on success. A
non-zero exit code from clang-tidy fails the product.

**Note:** This processor does not support batch mode. Each file is checked
separately to avoid cross-file analysis issues with unrelated files.

## Source Files

- Input: `{source_dir}/**/*.c`, `{source_dir}/**/*.cc`
- Output: `out/clang_tidy/{flat_name}.clang_tidy`

## Configuration

```toml
[processor.clang_tidy]
args = ["-checks=*"]                        # Arguments passed to clang-tidy
compiler_args = ["-std=c++17"]              # Arguments passed after -- to the compiler
dep_inputs = [".clang-tidy"]              # Additional files that trigger rebuilds when changed
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `args` | string[] | `[]` | Arguments passed to clang-tidy |
| `compiler_args` | string[] | `[]` | Compiler arguments passed after `--` separator |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

The tool processes one file at a time. Each file is checked in a separate invocation.

## Compiler Arguments

Clang-tidy requires knowing compiler flags to properly parse the source files.
Use `compiler_args` to specify include paths, defines, and language standards:

```toml
[processor.clang_tidy]
compiler_args = ["-std=c++17", "-I/usr/include/mylib", "-DDEBUG"]
```

## Using .clang-tidy File

Clang-tidy automatically reads configuration from a `.clang-tidy` file in the
project root. Add it to `dep_inputs` so changes trigger rebuilds:

```toml
[processor.clang_tidy]
dep_inputs = [".clang-tidy"]
```

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../processors.md#clean-behavior) and [`rsconstruct clean`](../commands.md#rsconstruct-clean).
