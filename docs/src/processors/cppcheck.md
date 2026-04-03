# Cppcheck Processor

## Purpose

Runs cppcheck static analysis on C/C++ source files.

## How It Works

Discovers `.c` and `.cc` files under the configured source directory, runs
cppcheck on each file individually, and creates a stub file on success. A
non-zero exit code from cppcheck fails the product.

**Note:** This processor does not support batch mode. Each file is checked
separately because cppcheck performs cross-file analysis (CTU - Cross Translation
Unit) which produces false positives when unrelated files are checked together.
For example, standalone example programs that define classes with the same name
will trigger `ctuOneDefinitionRuleViolation` errors even though the files are
never linked together. Cppcheck has no flag to disable this cross-file analysis
(`--max-ctu-depth=0` does not help), so files must be checked individually.

## Source Files

- Input: `{source_dir}/**/*.c`, `{source_dir}/**/*.cc`
- Output: `out/cppcheck/{flat_name}.cppcheck`

## Configuration

```toml
[processor.cppcheck]
args = ["--error-exitcode=1", "--enable=warning,style,performance,portability"]
extra_inputs = [".cppcheck-suppressions"]   # Additional files that trigger rebuilds when changed
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `args` | string[] | `["--error-exitcode=1", "--enable=warning,style,performance,portability"]` | Arguments passed to cppcheck |
| `extra_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

To use a suppressions file, add `"--suppressions-list=.cppcheck-suppressions"` to `args`.

## Batch support

The tool processes one file at a time. Each file is checked in a separate invocation.
