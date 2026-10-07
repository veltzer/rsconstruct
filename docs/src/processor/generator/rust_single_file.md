# Rust Single File Processor

## Purpose

Compiles single-file Rust programs (`.rs`) into executables, similar to the cc_single_file
processor but for Rust.

## How It Works

Rust source files in the `src/` directory are compiled directly to executables using `rustc`.
This is useful for exercise, example, or utility repositories where each `.rs` file is a
standalone program.

Output is written to `out/processor.generator.rust_single_file/` preserving the directory structure:

```text
src/hello.rs  →  out/processor.generator.rust_single_file/hello.elf
src/exercises/ex1.rs  →  out/processor.generator.rust_single_file/exercises/ex1.elf
```

## Source Files

- Input: `src/**/*.rs`
- Output: `out/processor.generator.rust_single_file/` with configured suffix (default: `.elf`)

## Configuration

```toml
[processor.generator.rust_single_file]
command = "rustc"                         # Rust compiler (default: "rustc")
flags = []                                # Additional compiler flags
output_suffix = ".elf"                    # Output file suffix (default: ".elf")
output_dir = "out/processor.generator.rust_single_file"       # Output directory
dep_inputs = []                         # Additional files that trigger rebuilds
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `command` | string | `"rustc"` | Path to Rust compiler |
| `flags` | string[] | `[]` | Additional compiler flags |
| `output_suffix` | string | `".elf"` | Suffix for output executables |
| `output_dir` | string | `"out/processor.generator.rust_single_file"` | Output directory |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

Each input file is processed individually, producing its own output file.

## Clean behavior

This processor is a Generator — `rsconstruct clean outputs` removes each declared output file individually with no directory recursion. After all per-product cleans complete, the orchestrator removes any parent directories that are now empty. Pass `--no-empty-dirs` to keep them. See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
