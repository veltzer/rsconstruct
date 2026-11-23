# Cppcheck Processor

## Purpose

Runs cppcheck static analysis on C/C++ source files.

## How It Works

Discovers `.c` and `.cc` files under the configured source directory, runs
cppcheck on each file individually, and creates a stub file on success. A
non-zero exit code from cppcheck fails the product.

**Note:** By default each file is checked separately (`batch = false`) because
cppcheck performs cross-file analysis (CTU - Cross Translation Unit), which
produces false positives when unrelated files are checked together. For
example, standalone example programs that define classes with the same name
will trigger `ctuOneDefinitionRuleViolation` errors even though the files are
never linked together. Cppcheck has no flag to disable this cross-file analysis
(`--max-ctu-depth=0` does not help), so unrelated files must not share a run.

The opposite problem is a multi-file program: checks such as
`--enable=unusedFunction` need every file of the program in one cppcheck run,
or a helper file's functions look unused. When the repo is a collection of
independent programs, one per directory, set `batch_by_dir = true`: cppcheck
then runs once per directory, on exactly that directory's files, and
directories are never mixed or split (see
[`batch_by_dir`](../../configuration.md#processortypename)). A change to any
file rechecks its whole directory.

```toml
[processor.checker.cppcheck]
batch_by_dir = true
args = ["--error-exitcode=1", "--enable=warning,style,performance,portability,information,unusedFunction", "--inline-suppr"]
```

## Source Files

- Input: `{source_dir}/**/*.c`, `{source_dir}/**/*.cc`
- Output: none (checker — a passing check is recorded in the cache, no file is written)

## Configuration

```toml
[processor.checker.cppcheck]
args = ["--error-exitcode=1", "--enable=warning,style,performance,portability"]
dep_inputs = [".cppcheck-suppressions"]   # Additional files that trigger rebuilds when changed
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `args` | string[] | `["--error-exitcode=1", "--enable=warning,style,performance,portability"]` | Arguments passed to cppcheck |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |
| `batch_by_dir` | boolean | `false` | One cppcheck run per directory, on exactly that directory's files (see above) |

To use a suppressions file, add `"--suppressions-list=.cppcheck-suppressions"` to `args`.

## Batch support

By default the tool processes one file at a time (`batch = false`). With `batch_by_dir = true` it runs once per directory; `batch_size` does not split a directory, and `--batch-size -1` disables the batching. A failing directory fails all of its files.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
