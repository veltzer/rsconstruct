# Configuration

RSConstruct is configured via an `rsconstruct.toml` file in the project root.

## Full reference

```toml
[build]
parallel = 1          # Number of parallel jobs (1 = sequential, 0 = auto-detect CPU cores)
batch_size = 0        # Max files per batch for batch-capable processors (0 = no limit, omit to disable)

# Declare processors by adding [processor.NAME] sections.
# Only declared processors run — no processors are enabled by default.
# Use `rsconstruct smart auto` to auto-detect and add relevant processors.

[processor.ruff]
# args = []

[processor.pylint]
# args = ["--disable=C0114"]

[processor.cc_single_file]
# cc = "gcc"
# cflags = ["-Wall", "-O2"]

[cache]
restore_method = "hardlink"  # or "copy" (hardlink is faster, copy works across filesystems)
remote = "s3://my-bucket/rsconstruct-cache"  # Optional: remote cache URL
remote_push = true       # Push local builds to remote (default: true)
remote_pull = true       # Pull from remote cache on cache miss (default: true)
mtime_check = true       # Use mtime pre-check to skip unchanged file checksums (default: true)

[analyzer]
auto_detect = true
enabled = ["cpp", "python"]

[graph]
viewer = "google-chrome"  # Command to open graph files (default: platform-specific)

[plugins]
dir = "plugins"  # Directory containing .lua processor plugins

[completions]
shells = ["bash"]
```

Per-processor configuration is documented on each processor's page under [Processors](processors.md).
Lua plugin configuration is documented under [Lua Plugins](plugins.md).

## Processor instances

Processors are declared by adding a `[processor.NAME]` section to `rsconstruct.toml`. An empty section enables the processor with default settings:

```toml
[processor.pylint]
```

Customize with config fields:

```toml
[processor.pylint]
args = ["--disable=C0114,C0116"]
scan_dir = "src"
```

Remove the section to disable the processor.

### Multiple instances

Run the same processor multiple times with different configurations by adding named sub-sections:

```toml
[processor.pylint.core]
scan_dir = "src/core"
args = ["--disable=C0114"]

[processor.pylint.tests]
scan_dir = "tests"
args = ["--disable=C0114,C0116"]
```

Each instance runs independently with its own config and cache. The instance name (e.g., `pylint.core`) appears in build output and cache keys.

You cannot mix single-instance and multi-instance formats for the same processor type — use either `[processor.pylint]` or `[processor.pylint.NAME]`, not both.

### Auto-detection

Run `rsconstruct smart auto` to scan the project and automatically add `[processor.NAME]` sections for all processors whose files are detected and whose tools are installed. It does not remove existing sections.

## Variable substitution

Define variables in a `[vars]` section and reference them using `${var_name}` syntax:

```toml
[vars]
kernel_excludes = ["/kernel/", "/kernel_standalone/", "/examples_standalone/"]

[processor.cppcheck]
exclude_dirs = "${kernel_excludes}"

[processor.cc_single_file]
exclude_dirs = "${kernel_excludes}"
```

Variables are substituted before TOML parsing. The `"${var_name}"` (including quotes) is replaced with the TOML-serialized value, preserving types (arrays stay arrays, strings stay strings). Undefined variable references produce an error.

## Section details

### `[build]`

| Key | Type | Default | Description |
|---|---|---|---|
| `parallel` | integer | `1` | Number of parallel jobs. `1` = sequential, `0` = auto-detect CPU cores. |
| `batch_size` | integer | `0` | Maximum files per batch for batch-capable processors. `0` = no limit (all files in one batch). Omit to disable batching entirely. |

### `[processor.NAME]`

Each `[processor.NAME]` section declares a processor instance. The section name must match a builtin processor type (e.g., `ruff`, `pylint`, `cc_single_file`) or a [Lua plugin](plugins.md) name.

Common fields available to all processors:

| Key | Type | Default | Description |
|---|---|---|---|
| `args` | array of strings | `[]` | Extra command-line arguments passed to the tool. |
| `extra_inputs` | array of strings | `[]` | Additional input files that trigger rebuild when changed. |
| `auto_inputs` | array of strings | varies | Config files auto-detected as inputs (e.g., `.pylintrc`). |
| `batch` | boolean | `true` | Whether to batch multiple files into a single tool invocation. |
| `scan_dir` | string | varies | Directory to scan for source files (empty = project root). |
| `extensions` | array of strings | varies | File extensions to match. |
| `exclude_dirs` | array of strings | varies | Directory path segments to exclude from scanning. |
| `exclude_files` | array of strings | `[]` | File names to exclude. |
| `exclude_paths` | array of strings | `[]` | Paths (relative to project root) to exclude. |

Processor-specific fields are documented on each processor's page under [Processors](processors.md).

### `[cache]`

| Key | Type | Default | Description |
|---|---|---|---|
| `restore_method` | string | `"hardlink"` | How to restore cached outputs. `"hardlink"` is faster; `"copy"` works across filesystems. |
| `remote` | string | none | Remote cache URL. See [Remote Caching](remote-caching.md). |
| `remote_push` | boolean | `true` | Push locally built artifacts to remote cache. |
| `remote_pull` | boolean | `true` | Pull from remote cache on local cache miss. |
| `mtime_check` | boolean | `true` | Use mtime pre-check to skip unchanged file checksums. |

### `[analyzer]`

| Key | Type | Default | Description |
|---|---|---|---|
| `auto_detect` | boolean | `true` | When `true`, only run enabled analyzers that auto-detect relevant files. |
| `enabled` | array of strings | `["cpp", "python"]` | List of dependency analyzers to enable. |

### `[graph]`

| Key | Type | Default | Description |
|---|---|---|---|
| `viewer` | string | platform-specific | Command to open graph files |

### `[plugins]`

| Key | Type | Default | Description |
|---|---|---|---|
| `dir` | string | `"plugins"` | Directory containing `.lua` processor plugins |

### `[completions]`

| Key | Type | Default | Description |
|---|---|---|---|
| `shells` | array | `["bash"]` | Shells to generate completions for |
