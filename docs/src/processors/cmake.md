# CMake Processor

## Purpose

Lints CMake files using `cmake --lint`.

## How It Works

Discovers `CMakeLists.txt` files in the project (excluding common build tool
directories), runs `cmake --lint` on each file, and records success in the cache.
A non-zero exit code from cmake fails the product.

This processor supports batch mode.

## Source Files

- Input: `**/CMakeLists.txt`
- Output: none (checker)

## Configuration

```toml
[processor.cmake]
args = []
extra_inputs = []
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `args` | string[] | `[]` | Extra arguments passed to cmake |
| `extra_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

The tool accepts multiple files on the command line. When batching is enabled (default), rsconstruct passes all files in a single invocation for better performance.
