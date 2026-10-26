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
- Output: none (checker — a passing check is recorded in the cache, no file is written)

## Configuration

```toml
[processor.checker.clang_tidy]
args = ["-checks=*"]                        # Arguments passed to clang-tidy
compiler_args = ["-std=c++17"]              # Arguments passed after -- to the compiler
dep_inputs = []                           # Additional files that trigger rebuilds when changed
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `args` | string[] | `[]` | Arguments passed to clang-tidy |
| `compiler_args` | string[] | `[]` | Compiler arguments passed after `--` separator |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |
| `dep_auto` | string[] | `[".clang-tidy"]` | Config files that are inputs when they exist |

## Batch support

The tool processes one file at a time. Each file is checked in a separate invocation.

## Compiler Arguments

Clang-tidy requires knowing compiler flags to properly parse the source files.
Use `compiler_args` to specify include paths, defines, and language standards:

```toml
[processor.checker.clang_tidy]
compiler_args = ["-std=c++17", "-I/usr/include/mylib", "-DDEBUG"]
```

## Using .clang-tidy File

Clang-tidy reads its configuration from the `.clang-tidy` file it finds in the
source file's parent directories. The project root's `.clang-tidy` is an input
of every product when it exists (`dep_auto`), so an edit to it re-checks every
file with no `dep_inputs` entry needed. A `.clang-tidy` in a subdirectory, or a
file named with `--config-file` in `args`, is not tracked; list it in
`dep_inputs`.

`compile_commands.json` is not an input: the processor always passes `--`
with `compiler_args`, and with `--` clang-tidy uses that fixed set of flags
instead of looking for a compilation database.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
