# Script Processor

## Purpose

Runs a user-configured script or command as a linter on discovered files. This
is a generic linter that lets you plug in any script without writing a custom
processor.

## How It Works

Discovers files matching the configured extensions in the configured scan
directory, then runs the configured linter command on each file (or batch of
files). A non-zero exit code from the script fails the product.

The `command` field has no default: declaring the processor without one is an
error when it runs.

This processor supports batch mode, allowing multiple files to be checked in a
single invocation for better performance.

## Source Files

- Input: configured via `src_extensions` and `src_dirs`
- Output: none (checker)

## Configuration

```toml
[processor.checker.script]
command = "python"
args = ["scripts/md_lint.py", "-q"]
src_extensions = [".md"]
src_dirs = ["marp"]
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `enabled` | bool | `true` | Set to `false` to keep the stanza but skip the processor |
| `command` | string | (required) | The command to run |
| `args` | string[] | `[]` | Extra arguments passed before file paths |
| `src_extensions` | string[] | `[]` | File extensions to scan for |
| `src_dirs` | string[] | `[""]` | Directory to scan (empty = project root) |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |
| `dep_auto` | string[] | `[]` | Auto-detected input files |
| `fix_command` | string | `""` | Command `rsconstruct fix` runs on the files; empty means the instance cannot fix |
| `fix_args` | string[] | `[]` | Arguments passed to `fix_command` before the file paths |
| `fix_batch` | bool | same as `batch` | Whether `fix_command` accepts several files per invocation |

## Fix mode

A script instance with a `fix_command` is fix-capable: `rsconstruct fix run`
runs `fix_command fix_args... <files>` on the same files the check covers,
and the fixer is expected to modify them in place. This is currently the only
way to make a processor fix-capable — no built-in processor declares a fixer.

```toml
[processor.checker.script.trailing]
command = "sh"
args = ["-c", "! grep -n -E ' +$' \"$@\"", "sh"]   # fails when a line ends in spaces
src_extensions = [".txt"]
src_dirs = ["src"]
fix_command = "sed"
fix_args = ["-i", "-E", "s/ +$//"]
```

```bash
rsconstruct fix list                                     # shows processor.checker.script.trailing
rsconstruct fix run processor.checker.script.trailing    # strips trailing spaces in place
```

## Batch support

The tool accepts multiple files on the command line. When batching is enabled (default), rsconstruct passes all files in a single invocation for better performance.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
