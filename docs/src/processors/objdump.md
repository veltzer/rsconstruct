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
extra_inputs = []
output_dir = "out/objdump"
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `args` | string[] | `[]` | Extra arguments passed to objdump |
| `extra_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |
| `output_dir` | string | `"out/objdump"` | Directory for disassembly output |

## Batch support

Each input file is processed individually, producing its own output file.
