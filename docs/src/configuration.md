# Configuration

RSConstruct is configured via an `rsconstruct.toml` file in the project root.

## Full reference

```toml
[build]
parallel = 1          # Number of parallel jobs (1 = sequential, 0 = auto-detect CPU cores)
                      # Also settable via RSCONSTRUCT_THREADS env var (CLI -j takes precedence)
batch_size = 0        # Max files per batch for batch-capable processors (0 = no limit, omit to disable)
output_dir = "out"    # Global output directory prefix for generator processors

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

[vars]
my_excludes = ["/vendor/", "/third_party/"]  # Define variables for reuse with ${var_name}

[cache]
restore_method = "auto"  # auto (default: copy in CI, hardlink otherwise), hardlink, or copy
compression = false      # Compress cached objects with zstd (requires restore_method = "copy")
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

[dependencies]
pip = ["pyyaml", "jinja2"]    # Python packages
npm = ["eslint", "prettier"]  # Node.js packages
gem = ["mdl"]                 # Ruby gems
system = ["pandoc", "graphviz"]  # System packages (checked but not auto-installed)
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
src_dirs = ["src"]
```

Remove the section to disable the processor.

### Multiple instances

Run the same processor multiple times with different configurations by adding named sub-sections:

```toml
[processor.pylint.core]
src_dirs = ["src/core"]
args = ["--disable=C0114"]

[processor.pylint.tests]
src_dirs = ["tests"]
args = ["--disable=C0114,C0116"]
```

Each instance runs independently with its own config and cache.

You cannot mix single-instance and multi-instance formats for the same processor type — use either `[processor.pylint]` or `[processor.pylint.NAME]`, not both.

#### Instance naming

A single instance declared as `[processor.pylint]` has the instance name `pylint`. Named instances declared as `[processor.pylint.core]` and `[processor.pylint.tests]` have instance names `pylint.core` and `pylint.tests`.

The instance name is used everywhere a processor is identified:

- **Build output and progress**: `[pylint.core] src/core/main.py`
- **Error messages**: `Error: [pylint.tests] tests/test_foo.py: ...`
- **Build statistics**: each instance reports its own file counts and durations
- **Cache keys**: instances have separate caches, so changing one config does not invalidate the other
- **Output directories**: generator processors default to `out/{instance_name}` (e.g., `out/marp.slides` and `out/marp.docs` for two marp instances), ensuring outputs do not collide
- **The `--processors` filter**: use the full instance name, e.g., `rsconstruct build -p pylint.core`

For single instances, the instance name equals the processor type name (e.g., `pylint`), so there is no visible difference from previous behavior.

### Auto-detection

Run `rsconstruct smart auto` to scan the project and automatically add `[processor.NAME]` sections for all processors whose files are detected and whose tools are installed. It does not remove existing sections.

## Variable substitution

Define variables in a `[vars]` section and reference them using `${var_name}` syntax:

```toml
[vars]
kernel_excludes = ["/kernel/", "/kernel_standalone/", "/examples_standalone/"]

[processor.cppcheck]
src_exclude_dirs = "${kernel_excludes}"

[processor.cc_single_file]
src_exclude_dirs = "${kernel_excludes}"
```

Variables are substituted before TOML parsing. The `"${var_name}"` (including quotes) is replaced with the TOML-serialized value, preserving types (arrays stay arrays, strings stay strings). Undefined variable references produce an error.

## Section details

### `[build]`

| Key | Type | Default | Description |
|---|---|---|---|
| `parallel` | integer | `1` | Number of parallel jobs. `1` = sequential, `0` = auto-detect CPU cores. Can also be set via the `RSCONSTRUCT_THREADS` environment variable (CLI `-j` takes precedence). |
| `batch_size` | integer | `0` | Maximum files per batch for batch-capable processors. `0` = no limit (all files in one batch). Omit to disable batching entirely. |
| `output_dir` | string | `"out"` | Global output directory prefix. Processor `output_dir` defaults that start with `out/` are remapped to use this prefix (e.g., setting `"build"` changes `out/marp` to `build/marp`). Individual processors can still override their `output_dir` explicitly. |

The `output_dir` prefix is purely a layout choice — `rsconstruct clean outputs` does not special-case it. Cleanup is driven by per-product `outputs` and `output_dirs` declarations, then a generic empty-directory sweep walks parents bottom-up. See [Clean behavior](processors.md#clean-behavior) and [`rsconstruct clean`](commands.md#rsconstruct-clean) for details.

### `[processor.NAME]`

Each `[processor.NAME]` section declares a processor instance. The section name must match a builtin processor type (e.g., `ruff`, `pylint`, `cc_single_file`) or a [Lua plugin](plugins.md) name.

Common fields available to all processors:

| Key | Type | Default | Description |
|---|---|---|---|
| `enabled` | boolean | `true` | Set to `false` to disable this processor without removing the stanza. Accepted on every processor. |
| `cache` | boolean | `true` | Whether to cache this processor's outputs. Set to `false` to always rebuild and never store results. Accepted on every processor. |
| `args` | array of strings | `[]` | Extra command-line arguments passed to the tool. |
| `dep_inputs` | array of strings | `[]` | Additional input files that trigger rebuild when changed. |
| `dep_auto` | array of strings | varies | Config files auto-detected as inputs (e.g., `.pylintrc`). |
| `batch` | boolean | `true` | Whether to batch multiple files into a single tool invocation. Note: in fail-fast mode (default), chunk size is 1 regardless of this setting — batch mode only groups files with `--keep-going` or `--batch-size`. For external tools, a batch failure marks all products in the chunk as failed. Internal processors (`i`-prefixed) return per-file results, so partial failure is handled correctly. |
| `max_jobs` | integer | none | Maximum concurrent jobs for this processor. When set, limits how many instances of this processor run in parallel, regardless of the global `-j` setting. Useful for heavyweight processors (e.g., `marp` spawns Chromium). Omit to use the global parallelism. |
| `src_dirs` | array of strings | varies | Directories to scan for source files. **Required** for most processors (defaults to `[]`). Processors with a specific default (e.g., `tera` defaults to `"tera.templates"`, `cc_single_file` defaults to `"src"`) do not require this. Not required when `src_files` is set. Use `rsconstruct processors defconfig <name>` to see a processor's defaults. |
| `src_extensions` | array of strings | varies | File extensions to match. |
| `src_exclude_dirs` | array of strings | varies | Directory path segments to exclude from scanning. |
| `src_exclude_files` | array of strings | `[]` | File names to exclude. |
| `src_exclude_paths` | array of strings | `[]` | Paths (relative to project root) to exclude. |
| `src_files` | array of strings | `[]` | When non-empty, only these exact paths are matched — `src_dirs`, `src_extensions`, and exclude filters are bypassed. Useful for processors that operate on specific files rather than scanning directories. |

Processor-specific fields are documented on each processor's page under [Processors](processors.md).

### `[cache]`

| Key | Type | Default | Description |
|---|---|---|---|
| `restore_method` | string | `"auto"` | How to restore cached outputs. `"auto"` (default) uses `"copy"` in CI environments (`CI=true`) and `"hardlink"` otherwise. `"hardlink"` is faster but requires same filesystem; `"copy"` works everywhere. |
| `compression` | boolean | `false` | Compress cached objects with zstd. Incompatible with `restore_method = "hardlink"` — requires `"copy"`. |
| `remote` | string | none | Remote cache URL. See [Remote Caching](remote-caching.md). |
| `remote_push` | boolean | `true` | Push locally built artifacts to remote cache. |
| `remote_pull` | boolean | `true` | Pull from remote cache on local cache miss. |
| `mtime_check` | boolean | `true` | Persist file checksums across builds using an mtime database. Set to `false` in CI/CD environments where the cache won't survive the build and the write overhead isn't worth it. Can also be disabled via `--no-mtime-cache` flag. See [Checksum Cache](internal/checksum-cache.md). |

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

### `[dependencies]`

Declare project dependencies by package manager. Used by `rsconstruct doctor` to verify availability and `rsconstruct tools install-deps` to install missing packages.

| Key | Type | Default | Description |
|---|---|---|---|
| `pip` | array of strings | `[]` | Python packages to install via `pip install`. Supports version specifiers (e.g., `"ruff>=0.4"`). |
| `npm` | array of strings | `[]` | Node.js packages to install via `npm install`. |
| `gem` | array of strings | `[]` | Ruby gems to install via `gem install`. |
| `system` | array of strings | `[]` | System packages installed via the detected package manager (`apt-get`, `dnf`, `pacman`, or `brew`). |
