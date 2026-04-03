# Checkpatch Processor

## Purpose

Checks C source files using the Linux kernel's `checkpatch.pl` script.

## How It Works

Discovers `.c` and `.h` files under `src/` (excluding common C/C++ build
directories), runs `checkpatch.pl` on each file, and records success in the
cache. A non-zero exit code from checkpatch fails the product.

This processor supports batch mode.

## Source Files

- Input: `src/**/*.c`, `src/**/*.h`
- Output: none (checker)

## Configuration

```toml
[processor.checkpatch]
args = []
extra_inputs = []
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `args` | string[] | `[]` | Extra arguments passed to checkpatch.pl |
| `extra_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

The tool processes one file at a time. Each file is checked in a separate invocation.
