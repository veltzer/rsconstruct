# Configuration

RSB is configured via an `rsb.toml` file in the project root.

## Full reference

```toml
[build]
parallel = 1  # Number of parallel jobs (1 = sequential, 0 = auto-detect CPU cores)

[processor]
auto_detect = true
enabled = ["template", "ruff", "pylint", "cc_single_file", "cpplint", "spellcheck", "sleep", "make"]

[cache]
restore_method = "hardlink"  # or "copy" (hardlink is faster, copy works across filesystems)

[graph]
viewer = "google-chrome"  # Command to open graph files (default: platform-specific)

[completions]
shells = ["bash"]
```

Per-processor configuration is documented on each processor's page under [Processors](processors.md).

## Section details

### `[build]`

| Key | Type | Default | Description |
|---|---|---|---|
| `parallel` | integer | `1` | Number of parallel jobs. `1` = sequential, `0` = auto-detect CPU cores. |

### `[processor]`

| Key | Type | Default | Description |
|---|---|---|---|
| `auto_detect` | boolean | `true` | When `true`, only run enabled processors that auto-detect relevant files. When `false`, run all enabled processors unconditionally. |
| `enabled` | array of strings | all | List of processors to enable. Available: `template`, `ruff`, `pylint`, `cc_single_file`, `cpplint`, `spellcheck`, `sleep`, `make`. |

### `[cache]`

| Key | Type | Default | Description |
|---|---|---|---|
| `restore_method` | string | `"hardlink"` | How to restore cached outputs. `"hardlink"` is faster; `"copy"` works across filesystems. |

### `[graph]`

| Key | Type | Default | Description |
|---|---|---|---|
| `viewer` | string | platform-specific | Command to open graph files |

### `[completions]`

| Key | Type | Default | Description |
|---|---|---|---|
| `shells` | array | `["bash"]` | Shells to generate completions for |
