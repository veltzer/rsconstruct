# Objdump Processor

## Purpose

Disassembles ELF binaries using `objdump`.

## How It Works

Discovers `.elf` files under `out/cc_single_file/`, runs `objdump` to produce
disassembly output, and writes the result to the configured output directory.

## Source Files

- Input: `out/cc_single_file/**/*.elf`
- Output: disassembly files in output directory

## Configuration

```toml
[processor.objdump]
args = []
dep_inputs = []
output_dir = "out/objdump"
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `args` | string[] | `[]` | Extra arguments passed to objdump |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |
| `output_dir` | string | `"out/objdump"` | Directory for disassembly output |

## Batch support

Each input file is processed individually, producing its own output file.

## Clean behavior

This processor is a Generator — `rsconstruct clean outputs` removes each declared output file individually with no directory recursion. After all per-product cleans complete, the orchestrator removes any parent directories that are now empty. Pass `--no-empty-dirs` to keep them. See [Clean behavior](../processors.md#clean-behavior) and [`rsconstruct clean`](../commands.md#rsconstruct-clean).
