# Cpplint Processor

## Purpose

Runs static analysis on C/C++ source files using an external checker
(cppcheck by default).

## How It Works

Discovers `.c` and `.cc` files under the configured source directory, runs the
checker on each file, and creates a stub file on success. A non-zero exit code
from the checker fails the product.

## Source Files

- Input: `{source_dir}/**/*.c`, `{source_dir}/**/*.cc`
- Output: `out/cpplint/{flat_name}.cpplint`

## Configuration

```toml
[processor.cpplint]
checker = "cppcheck"                        # Static checker command (default: "cppcheck")
args = ["--error-exitcode=1", "--enable=warning,style,performance,portability"]
extra_inputs = [".cppcheck-suppressions"]   # Additional files that trigger rebuilds when changed
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `checker` | string | `"cppcheck"` | The checker executable to invoke |
| `args` | string[] | `["--error-exitcode=1", "--enable=warning,style,performance,portability"]` | Arguments passed to the checker |
| `extra_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

To use a suppressions file, add `"--suppressions-list=.cppcheck-suppressions"` to `args`.
