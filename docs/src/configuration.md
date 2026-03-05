# Configuration

RSBuild is configured via an `rsbuild.toml` file in the project root.

## Full reference

```toml
[build]
parallel = 1          # Number of parallel jobs (1 = sequential, 0 = auto-detect CPU cores)
batch_size = 0        # Max files per batch for batch-capable processors (0 = no limit, omit to disable)

[processor]
auto_detect = true
enabled = ["tera", "ruff", "pylint", "mypy", "pyrefly", "cc_single_file", "cppcheck",
           "clang_tidy", "shellcheck", "spellcheck", "make", "cargo", "rumdl", "yamllint",
           "jq", "jsonlint", "taplo", "json_schema", "tags", "pip", "sphinx", "mdbook",
           "npm", "gem", "mdl", "markdownlint", "aspell", "marp", "pandoc", "markdown",
           "pdflatex", "a2x", "ascii_check", "mermaid", "drawio", "libreoffice", "pdfunite"]

[cache]
restore_method = "hardlink"  # or "copy" (hardlink is faster, copy works across filesystems)
remote = "s3://my-bucket/rsbuild-cache"  # Optional: remote cache URL
remote_push = true       # Push local builds to remote (default: true)
remote_pull = true       # Pull from remote on cache miss (default: true)
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

### `[processor]`

| Key | Type | Default | Description |
|---|---|---|---|
| `auto_detect` | boolean | `true` | When `true`, only run enabled processors that auto-detect relevant files. When `false`, run all enabled processors unconditionally. |
| `enabled` | array of strings | (see below) | List of processors to enable. By default all built-in processors are enabled. Run `rsbuild processors list` to see the full list. [Lua plugin](plugins.md) names can also be listed here. |

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
