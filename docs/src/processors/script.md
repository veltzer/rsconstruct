# Script Processor

## Purpose

Runs a user-configured script or command as a linter on discovered files. This
is a generic linter that lets you plug in any script without writing a custom
processor.

## How It Works

Discovers files matching the configured extensions in the configured scan
directory, then runs the configured linter command on each file (or batch of
files). A non-zero exit code from the script fails the product.

This processor is **disabled by default** — you must set `enabled = true` and
provide a `linter` command in your `rsconstruct.toml`.

This processor supports batch mode, allowing multiple files to be checked in a
single invocation for better performance.

## Source Files

- Input: configured via `extensions` and `scan_dir`
- Output: none (linter)

## Configuration

```toml
[processor.script]
enabled = true
linter = "python"
args = ["scripts/md_lint.py", "-q"]
extensions = [".md"]
scan_dir = "marp"
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `enabled` | bool | `false` | Must be set to `true` to activate |
| `linter` | string | `""` | The command to run (required) |
| `args` | string[] | `[]` | Extra arguments passed before file paths |
| `extensions` | string[] | `[]` | File extensions to scan for |
| `scan_dir` | string | `""` | Directory to scan (empty = project root) |
| `extra_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |
| `auto_inputs` | string[] | `[]` | Auto-detected input files |
