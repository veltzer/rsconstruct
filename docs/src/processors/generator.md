# Generator Processor

## Purpose

Runs a user-configured script or command as a generator, producing output files
from input files. The script receives input/output path pairs on the command line.

## How It Works

Discovers files matching the configured extensions, computes output paths under
`output_dir` with the configured `output_extension`, and invokes the command with
path pairs.

In single mode: `command [args...] <input> <output>`

In batch mode: `command [args...] <input1> <output1> <input2> <output2> ...`

Auto-detected when the configured scan directories contain matching files.

## Source Files

- Input: files matching `extensions` in `scan_dirs`
- Output: `{output_dir}/{relative_path}.{output_extension}`

## Configuration

```toml
[processor.generator]
command = "scripts/convert.py"
output_dir = "out/converted"
output_extension = "html"
scan_dirs = ["syllabi"]
extensions = [".md"]
batch = true
args = []
extra_inputs = []
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `command` | string | `"true"` | Script or command to run |
| `output_dir` | string | `"out/generator"` | Directory for output files |
| `output_extension` | string | `"out"` | Extension for output files |
| `batch` | bool | `true` | Pass all pairs in one invocation |
| `args` | string[] | `[]` | Extra arguments prepended before file pairs |
| `extra_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

Configurable via `batch = true` (default). In batch mode, the script receives all input/output pairs in a single invocation. Set `batch = false` to invoke the script once per file.
