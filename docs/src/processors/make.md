# Make Processor

## Purpose

Runs `make` in directories containing Makefiles. Each Makefile produces a stub
file on success, allowing RSB to track incremental rebuilds.

## How It Works

Discovers files named `Makefile` in the project. For each Makefile found, the
processor runs `make` (or a configured alternative) in the Makefile's directory.
A stub file is created on success.

## Source Files

- Input: `**/Makefile`
- Output: `out/make/{relative_path}.done`

## Configuration

```toml
[processor.make]
make = "make"        # Make binary to use
args = []            # Extra arguments passed to make
target = ""          # Make target (empty = default target)
scan_dir = ""        # Directory to scan ("" = project root)
extensions = ["Makefile"]
extra_inputs = []    # Additional files that trigger rebuilds when changed
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `make` | string | `"make"` | Path or name of the make binary |
| `args` | string[] | `[]` | Extra arguments passed to every make invocation |
| `target` | string | `""` | Make target to build (empty = default target) |
| `scan_dir` | string | `""` | Directory to scan for Makefiles |
| `extensions` | string[] | `["Makefile"]` | File names to match |
| `extra_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |
