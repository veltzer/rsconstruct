# Configuration

RSConstruct is configured via an `rsconstruct.toml` file in the project root.
An optional `rsconstruct.local.toml` overlay, when present, is merged over the
main file at load time — see [Local overlay](#local-overlay-rsconstructlocaltoml).
An optional user-level file can set `[build]` defaults underneath both — see
[User config](#user-config-configrsconstructconfigtoml).

## Full reference

```toml
[build]
parallel = 1          # Number of parallel jobs (1 = sequential, 0 = auto-detect CPU cores)
                      # Also settable via RSCONSTRUCT_THREADS env var (CLI -j takes precedence)
batch_size = 0        # Max files per batch for batch-capable processors (0 = no limit)
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
webcache_ttl_secs = 604800  # How long a fetched HTTP response stays fresh (default: 7 days)

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

[pages]
dir = "out/web"  # Directory published to GitHub Pages (omit the section entirely for non-Pages repos)
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

Variables are substituted before TOML parsing. The `"${var_name}"` (including quotes) is replaced with the TOML-serialized value, preserving types (arrays stay arrays, strings stay strings). A reference must constitute the **entire quoted value** — an embedded reference like `"${base}/src"` cannot be substituted and is a config error, as is any undefined variable reference.

## Local overlay (`rsconstruct.local.toml`)

If a file named `rsconstruct.local.toml` exists next to `rsconstruct.toml`, it
is loaded automatically and deep-merged over the main config. This lets many
repos share one canonical, identical `rsconstruct.toml` while keeping
repo-specific configuration — `[dependencies]` lists, `[processor.explicit.*]`
generators, exclude lists, one-off workarounds — in the local file.

Merge semantics:

- **Tables merge recursively.** A local `[processor.mypy]` with only
  `batch = false` adds that one key to the main file's mypy section; the rest
  of the section is untouched.
- **Arrays and scalars replace wholesale.** If the local file sets
  `src_dirs`, that exact list is used — values are never concatenated, so the
  local file can also *remove* entries by setting a shorter list.
- **Local-only sections are added.** The overlay can declare whole processors,
  analyzers, or global sections the main file doesn't have.

Rules and caveats:

- The overlay only extends a main config: `rsconstruct.local.toml` without an
  `rsconstruct.toml` is an error.
- `[vars]` substitution is per-file — each file's `${...}` references resolve
  against its own `[vars]` section only.
- The merged result is validated as one config, so unknown fields or type
  errors in the local file are reported the same way as in the main file.
- `rsconstruct processors config <iname>` shows the merged values.
- In watch mode, both files are watched for changes.

This supports a shared-config pattern: one `rsconstruct.toml` distributed
across many repos plus a small optional `rsconstruct.local.toml` per repo for
what's genuinely repo-specific. Note that every `src_dirs` entry must exist in
every repo that carries it (see [Missing `src_dirs` and `src_files`
entries](#missing-src_dirs-and-src_files-entries)); a shared file can no longer
list directories only some repos have, and `[build] allow_missing_src_dirs`
exists for a project that still wants that.

## User config (`~/.config/rsconstruct/config.toml`)

A per-user file, read from `$XDG_CONFIG_HOME/rsconstruct/config.toml` (or
`~/.config/rsconstruct/config.toml` when `XDG_CONFIG_HOME` is unset), sets
`[build]` defaults for every project you build on this machine. Precedence,
lowest first:

1. built-in defaults
2. the user config
3. `rsconstruct.toml`
4. `rsconstruct.local.toml`

Keys merge one by one, so a repo that sets a `[build]` key overrides the user
value for that key only. Example:

```toml
# ~/.config/rsconstruct/config.toml
[build]
reject_dot_src_dirs = true
```

Rules:

- **Only `[build]` is allowed.** A `[processor.*]`, `[analyzer.*]` or any
  other table in this file is a config error naming the offending section.
  Anything that changes what a repo scans or builds belongs in the repo's own
  files, where every machine and CI see the same thing; a machine-wide file
  that could add processors would make the same repo build differently on
  two machines.
- **CI never reads it.** Continuous integration checks out the repo and
  nothing else, so a switch that must hold in CI has to be set in
  `rsconstruct.toml`. The user file is a local safety net and a way to try a
  policy across all your projects before committing it to each of them.
- It applies even in a directory without `rsconstruct.toml`, for the commands
  that run without a config.
- Watch mode does not watch this file; restart the watcher after editing it.
- `rsconstruct toml files` prints the resolved path of this file, together
  with the rest of the chain, and whether each one exists.

## Section details

### `[build]`

| Key | Type | Default | Description |
|---|---|---|---|
| `parallel` | integer | `1` | Number of parallel jobs. `1` = sequential, `0` = auto-detect CPU cores. Can also be set via the `RSCONSTRUCT_THREADS` environment variable (CLI `-j` takes precedence). |
| `batch_size` | integer | `0` | Maximum files per batch for batch-capable processors. `0` = no limit (all files in one batch). To disable batching, pass `--batch-size -1` on the CLI or set `batch = false` on individual processors. |
| `output_dir` | string | `"out"` | Global output directory prefix. Processor `output_dir` defaults that start with `out/` are remapped to use this prefix (e.g., setting `"build"` changes `out/marp` to `build/marp`). Individual processors can still override their `output_dir` explicitly. |
| `max_discovery_passes` | integer | `10` | Maximum fixed-point discovery passes. Discovery repeats while processors keep adding products from each other's declared outputs; a config still adding products at the cap fails the build (naming the processors involved) instead of silently truncating the graph. |
| `hash_tool_versions` | boolean | `true` | Mix the identity of each processor's external tools into its products' cache keys, so upgrading a tool invalidates what that tool produced. Tools are identified by a content hash of the resolved binary (no `--version` subprocess; the mtime cache makes repeat builds a stat), or by the pinned `version_output` when [`rsconstruct tools lock`](commands.md#rsconstruct-tools) has written a `.tools.versions` file. Set to `false` to leave tool identity out of cache keys entirely. |
| `warn_symlinks` | boolean | `false` | Warn about every symlink skipped during the file-index walk. The walker never follows symlinks, so a symlinked source (or a symlinked directory of sources) is absent from the index — never checked, never built. Set to `true` to make that gap visible. Off by default because projects that deliberately keep symlinks around (vendored trees, dotfile farms) would drown in warnings. |
| `command_timeout_secs` | integer | `0` | Wall-clock limit in seconds for every external command a processor runs; a command still running at the limit is killed and its product fails with `Command timed out after Ns and was killed`. `0` means no limit. **Off by default, on purpose**: a tool that hangs is a bug in the tool, its input, or the environment, and the fix is to find that cause — not to cut the tool off. Set this only where a build must not be allowed to sit forever (an unattended runner) and a loud kill beats a silent hang. A processor with its own timeout (`marp`'s `timeout_secs`) keeps it; the explicit value wins. |
| `allow_missing_dep_auto` | boolean | `false` | Skip a `dep_auto` entry you listed when the file does not exist, as every entry used to be skipped. By default such an entry is a config error naming the processor, the file and the config line: a listed file that is missing is a typo or a stale copy of a shared config, and either way a dependency that silently tracks nothing. Processor defaults (`.pylintrc`, `ruff.toml`, ...) are optional by nature and are never checked. |
| `allow_missing_src_dirs` | boolean | `false` | Skip a `src_dirs` entry that names a directory which does not exist, as every entry used to be skipped. By default such an entry is a config error naming the processor and the entry: a directory listed in the config that is not there is a typo, a stanza left behind when the directory moved, or a copy of another repo's config, and in every case a processor that checks nothing while the build stays green. A directory an upstream processor declares as its output is never reported, whatever this is set to. This switch covers directories only: a missing `src_files` entry is always an error. |
| `reject_dot_src_dirs` | boolean | `false` | Make a `src_dirs` entry of `"."` a config error. `"."` and `""` both mean the whole project tree, recursively; written as `"."` it reads like "the root directory" and hides that the stanza sweeps every subdirectory too. Turn this on in a project whose stanzas are meant to name the directories they cover: the error names the stanza and the config line, and the fix is to list the real directories, or to write `""` if sweeping the tree is intended. Off by default because `"."` is not wrong, only easy to misread. Disabled stanzas are not checked. |

The `output_dir` prefix is purely a layout choice — `rsconstruct clean outputs` does not special-case it. Cleanup is driven by per-product `outputs` and `output_dirs` declarations, then a generic empty-directory sweep walks parents bottom-up. See [Clean behavior](processors.md#clean-behavior) and [`rsconstruct clean`](commands.md#rsconstruct-clean) for details.

### `[processor.NAME]`

Each `[processor.NAME]` section declares a processor instance. The section name must match a builtin processor type (e.g., `ruff`, `pylint`, `cc_single_file`) or a [Lua plugin](plugins.md) name.

Common fields available to all processors:

| Key | Type | Default | Description |
|---|---|---|---|
| `enabled` | boolean | `true` | Set to `false` to disable this processor without removing the stanza. Accepted on every processor. A disabled processor is skipped during discovery, so it produces no products; `smart remove-no-file-processors` ignores disabled stanzas rather than reporting them as matching no files. |
| `args` | array of strings | `[]` | Extra command-line arguments passed to the tool. |
| `dep_inputs` | array of strings | `[]` | Additional input files that trigger rebuild when changed. |
| `dep_auto` | array of strings | varies | Config files added as inputs. The processor's default list (e.g. `.pylintrc`) is skip-if-absent; a list you write here replaces it, and every entry in it must exist unless `[build] allow_missing_dep_auto` is set. |
| `required_tools` | array of strings | `[]` | Extra tools this processor needs beyond `command`. Normally `command` *is* the tool, so this is empty. Name the real tool here when `command` is a wrapper script that shells out to it — otherwise that tool is invisible to `rsconstruct tools install` and to version locking, and a missing tool surfaces only as a failure inside the wrapper. Each name must have a registry entry (`rsconstruct tools list`). |
| `batch` | boolean | `true` | Whether to batch multiple files into a single tool invocation. Note: in fail-fast mode (default), chunk size is 1 regardless of this setting — batch mode only groups files with `--keep-going` or `--batch-size`. For external tools, a batch failure marks all products in the chunk as failed. Internal processors (`i`-prefixed) return per-file results, so partial failure is handled correctly. |
| `max_jobs` | integer | none | Maximum concurrent jobs for this processor. When set, limits how many instances of this processor run in parallel, regardless of the global `-j` setting. Useful for heavyweight processors (e.g., `marp` spawns Chromium). Omit to use the global parallelism. |
| `src_dirs` | array of strings | `[]` | Directories to scan for source files. **Every processor defaults to `[]`, which scans nothing** — no processor guesses a directory, so one declared without `src_dirs` matches no files and simply builds nothing. A default like `["src"]` would be a guess that silently matches the wrong directory in a project laid out differently; naming the directory is the user's call. To scan the whole project deliberately, use `src_dirs = [""]` — an empty string means the project root and walks everything beneath it, including `node_modules/`, `.venv/` and `target/` unless excluded. `"."` means exactly the same thing, but reads as if it named only the root directory; `[build] reject_dot_src_dirs = true` forbids that spelling. `src_files` is an alternative to `src_dirs` for listing exact paths. **Every entry must name a directory that exists**; an entry that does not is a config error (exit code 2) naming the processor and the entry — see [Missing `src_dirs` entries](#missing-src_dirs-entries) below. The one exception is a directory an upstream processor declares as its output: it does not exist before that processor runs, and is accepted. Use `rsconstruct processors defconfig <name>` to see a processor's defaults. |
| `src_extensions` | array of strings | varies | File extensions to match. |
| `src_exclude_dirs` | array of strings | varies | Directory path segments to exclude from scanning. |
| `src_exclude_files` | array of strings | `[]` | File names to exclude. |
| `src_exclude_paths` | array of strings | `[]` | Paths (relative to project root) to exclude. |
| `src_files` | array of strings | `[]` | Exact paths to match, relative to the project root. On its own it is an allowlist: **exactly the named files are matched and nothing else** (no directory scan, no extension filter). Together with `src_dirs`, the named files are added to what the directories yield. **Every entry must name a file that exists**, or a path an upstream processor declares as its output; anything else is a config error (exit code 2), and unlike a missing directory it is never forgiven — `allow_missing_src_dirs` does not apply to files. See [Missing `src_dirs` and `src_files` entries](#missing-src_dirs-and-src_files-entries). |

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
| `webcache_ttl_secs` | integer | `604800` (7 days) | How long a cached HTTP response (remote JSON schemas fetched by `iyamlschema`) stays fresh. An entry older than this is re-fetched. Set to `0` to disable the web cache entirely and always re-fetch. Expired entries are reclaimed by [`rsconstruct cache trim`](commands.md#rsconstruct-cache). |

`rsconstruct cache trim` reclaims three kinds of dead weight: unreferenced objects in the store, mtime-database rows for files that no longer exist, and expired web-cache entries. Without it these grow with the history of the project rather than its current size.

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

### Missing `src_dirs` and `src_files` entries

A `src_dirs` or `src_files` entry is a claim about where the project keeps its
sources. When the directory or file it names is not there, the build fails at
discovery with exit code 2 (`EXIT_CONFIG_ERROR`) and lists every offending
entry:

```
Invalid config:
  [processor.pylint] src_dirs entry 'scripts' does not exist or is not a directory
  [processor.luacheck] src_dirs entry 'config' does not exist or is not a directory
  [processor.taplo] src_files entry 'pyproject.toml' does not exist or is not a file
Every src_dirs entry must name a directory that exists and every src_files
entry a file that exists (or a path an upstream processor declares as its
output) — fix the path or remove the entry; [build] allow_missing_src_dirs =
true skips absent directories as before, never absent files
```

Directories used to be a silent skip, visible only under `--phases`, and files
were never checked at all. That let a stanza whose directory had moved, one
copied from another repo, or one with a typo in a file name keep the build
green while checking nothing at all — the worst kind of failure, because the
green tick says the check ran. The fix is always in the config: point the
entry at the directory or file that actually exists, or delete the stanza if
the files are gone.

What is **not** reported:

- `""` and `"."`, which mean the project root and always exist.
- A directory or file an upstream processor declares as its output (for
  example a linter whose `src_dirs` is a generator's `output_dir`, or whose
  `src_files` names one of its outputs). It does not exist before that
  processor has run; discovery sees the declared outputs as virtual files and
  accepts the entry.
- Entries of a stanza with `enabled = false`.
- Anything during `rsconstruct clean` and `rsconstruct smart
  remove-no-file-processors`: clean must still be able to remove the outputs
  of a build whose source directory has since gone, and the repair command
  exists to delete the very stanzas this check rejects. Both report the
  entries under `--phases` instead.

`[build] allow_missing_src_dirs = true` restores the old skip for directories
in a project that needs it; the skipped entries are then listed under
`--phases`. It never covers `src_files`: a directory can at least plausibly be
empty, a named file that is not there is simply wrong. Prefer fixing the
entry: the switch hides exactly the misconfiguration this check exists to
catch.

### `[dependencies]`

Declare project dependencies by package manager. Used by `rsconstruct doctor` to verify availability and `rsconstruct tools install-deps` to install missing packages.

| Key | Type | Default | Description |
|---|---|---|---|
| `pip` | array of strings | `[]` | Python packages to install via `pip install`. Supports version specifiers (e.g., `"ruff>=0.4"`). |
| `pip_source` | string | `"uv-lock"` | Where the Python package set comes from: `"uv-lock"` installs the pinned closure from `uv.lock`; `"pyproject"` resolves the names pyproject.toml declares at install time. |
| `python_installer` | string | `"uv"` | Which tool installs the Python package set: `"uv"` runs `uv pip install --python python3`; `"pip"` runs `pip install`. See below for why the two differ on the same lock. |
| `npm` | array of strings | `[]` | Node.js packages to install via `npm install`. |
| `gem` | array of strings | `[]` | Ruby gems to install via `gem install`. |
| `system` | array of strings | `[]` | System packages installed via the detected package manager (`apt-get`, `dnf`, `pacman`, or `brew`). |

#### Python dependencies come from `uv.lock` by default

In the default `pip_source = "uv-lock"` mode, `install-deps` and `doctor`
take the Python package set from `uv.lock`: the exact pinned closure —
including transitive dependencies — that `uv lock` resolved, so CI installs
are reproducible instead of floating to whatever pip resolves that day. Only
registry packages are read from the lock; the project's own entry is
skipped, and any other source kind (git, path) is an error. A project whose
`pyproject.toml` declares Python dependencies but has no `uv.lock` is an
error: run `uv lock`, or opt out per repo with `pip_source = "pyproject"`.

In `pip_source = "pyproject"` mode the set is instead the dependency *names*
pyproject.toml declares, resolved by pip at install time:

- `[project].dependencies`
- every list in `[project.optional-dependencies]`
- every [PEP 735](https://peps.python.org/pep-0735/) `[dependency-groups]`
  list (entries that are `{include-group = "..."}` tables are skipped, since
  every group is read anyway)

In both modes `[dependencies].pip` entries merge in front and the result is
deduplicated by PEP 503-normalized distribution name plus extras, so a
`[dependencies].pip` entry wins over a lock or pyproject one. Use
`[dependencies].pip` for repos without a `pyproject.toml` and for packages
CI needs that the project itself does not declare.

#### uv installs the Python set by default

With the default `python_installer = "uv"`, `install-deps` hands the Python
set to `uv pip install --python python3` — targeting whichever interpreter
`python3` on PATH resolves to, the same environment a bare `pip install`
would touch. Installing a uv-produced lock with uv matters for two reasons:

- **Requires-Python upper bounds.** uv deliberately ignores a package's
  declared *upper* Requires-Python bound when resolving (upper bounds are
  almost always stale metadata), so `uv lock` can pin a version whose bound
  excludes the running interpreter. uv installs that pin; pip enforces the
  bound and refuses it with "No matching distribution found".
- **Environment markers.** In uv-lock mode the pins come from `uv export`
  (run with `--frozen`, so an out-of-sync lock is an error, not a silent
  re-resolve), which annotates platform-conditional packages with markers
  like `pywin32==312 ; sys_platform == 'win32'`. uv skips a pin whose
  marker does not match the current platform, where the flattened lock
  closure would ask for another platform's package and fail.

uv also audits the whole installed set itself in one pass, so uv mode skips
the per-package `pip show` preflight probing.

When uv is not on PATH and there is Python work to do, `install-deps`
bootstraps it with `pip install uv` first — CI runner images ship pip but
not necessarily uv, and the consuming repo's workflow should not need to
know which installer rsconstruct uses.

Set `python_installer = "pip"` to keep the pre-uv behavior: per-package
`pip show` probing followed by `pip install` of the flattened set.

#### Install order

`rsconstruct tools install-deps` always installs in this fixed order:

1. **`system`** — OS packages (apt, dnf, pacman, brew)
2. **`pip`** — Python packages
3. **`npm`** — Node.js packages
4. **`gem`** — Ruby gems

This order is deliberate and must not be changed. Language-level packages frequently build native extensions that link against system libraries at install time. For example, installing `manim` via pip pulls in `manimpango`, which compiles a C extension and uses `pkg-config` to find `pangocairo` — so `libpango1.0-dev` must already be on the system before `pip install` runs. Running `pip` (or `gem`, or `npm`) before `system` causes wheel/extension builds to fail with messages like `Package 'pangocairo' was not found`.

The keys inside `[dependencies]` may appear in any order in `rsconstruct.toml`; the install order is enforced by `install-deps` regardless.

#### `eatmydata` wrapping

When [`eatmydata`](https://www.flamingspork.com/projects/libeatmydata/) is installed on the system *and* `CI=true` is in the environment, both `rsconstruct tools install` and `rsconstruct tools install-deps` wrap their `apt`/`dnf`/`pacman` invocations with it. `eatmydata` no-op's `fsync()` for the wrapped process, which speeds up package installs by 3–10×.

The trade-off is loss-on-power-cut: any package files written during the install are not flushed to disk, so a power loss mid-install can leave the package database inconsistent. That's fine on transient CI hosts and wrong on developer workstations — hence the `CI=true` gate.

The wrap inserts `eatmydata` *after* `sudo` so the `LD_PRELOAD` applies to the package manager, not to `sudo` itself: e.g. `sudo eatmydata apt-get install -y …`.

If `eatmydata` is not installed, the commands run unwrapped — no error, no warning.

##### Controlling the wrap

| To...                                       | Do this                                              |
| ------------------------------------------- | ---------------------------------------------------- |
| Use the wrap (the CI default)               | Set `CI=true` and have eatmydata installed           |
| Skip the wrap in CI                         | Unset `CI` (or set it to anything other than `true`) |
| Use the wrap outside CI                     | Run with `CI=true rsconstruct tools install-deps`    |
| Skip the wrap for a single invocation       | Pass `--no-eatmydata`                                |

The CLI flag `--no-eatmydata` always wins. Otherwise the policy is driven entirely by `CI=true`. There is no `rsconstruct.toml` field for this — the env var is the knob.

The mechanism is a [post-config hook](processors.md): `eatmydata_ci_default` runs after config load and flips the in-memory `dependencies.eatmydata` flag when `CI=true`. List it with `rsconstruct hooks`.

##### What's never wrapped

`brew` is never wrapped (eatmydata is Linux-only). `pip`, `npm`, `cargo`, and `gem` are never wrapped either — those don't fsync excessively, so the wrap adds nothing.

### `[pages]`

Declares that this repo publishes a directory to GitHub Pages. The section is optional — its *presence* is the signal. `rsconstruct pages dir` prints the directory when the section exists and prints nothing (exit 0) when it doesn't, which lets a single shared CI workflow decide whether to run the Pages upload/deploy steps. See [GitHub Actions](github-actions.md#github-pages-deployment) for the workflow pattern.

| Key | Type | Default | Description |
|---|---|---|---|
| `dir` | string | required | Directory whose contents are published to GitHub Pages (e.g., `"out/web"` or `"_site"`). |
