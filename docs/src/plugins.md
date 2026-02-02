# Lua Plugins

RSB supports custom processors written in Lua. Drop a `.lua` file in the `plugins/` directory and add its name to `[processor] enabled` in `rsb.toml`. The plugin participates in discovery, execution, caching, cleaning, tool listing, and auto-detection just like a built-in processor.

## Quick Start

**1. Create the plugin file:**

```
plugins/eslint.lua
```

```lua
function description()
    return "Lint JavaScript/TypeScript with ESLint"
end

function required_tools()
    return {"eslint"}
end

function discover(project_root, config, files)
    local products = {}
    for _, file in ipairs(files) do
        local stub = rsb.stub_path(project_root, file, "eslint")
        table.insert(products, {
            inputs = {file},
            outputs = {stub},
        })
    end
    return products
end

function execute(product)
    rsb.run_command("eslint", {product.inputs[1]})
    rsb.write_stub(product.outputs[1], "linted")
end
```

**2. Enable it in `rsb.toml`:**

```toml
[processor]
enabled = ["eslint"]

[processor.eslint]
scan_dir = "src"
extensions = [".js", ".ts"]
```

**3. Run it:**

```sh
rsb build            # builds including the plugin
rsb processor list   # shows the plugin
rsb processor files  # shows files discovered by the plugin
```

## Lua API Contract

Each `.lua` file defines global functions. Three are required; the rest have sensible defaults.

### Required Functions

#### `description()`

Returns a human-readable string describing what the processor does. Called once when the plugin is loaded.

```lua
function description()
    return "Lint JavaScript files with ESLint"
end
```

#### `discover(project_root, config, files)`

Called during product discovery. Receives:

- `project_root` (string) — absolute path to the project root
- `config` (table) — the `[processor.NAME]` TOML section as a Lua table
- `files` (table) — list of absolute file paths matching the scan configuration

Must return a table of products. Each product is a table with `inputs` and `outputs` keys, both containing tables of absolute file paths.

```lua
function discover(project_root, config, files)
    local products = {}
    for _, file in ipairs(files) do
        local stub = rsb.stub_path(project_root, file, "myplugin")
        table.insert(products, {
            inputs = {file},
            outputs = {stub},
        })
    end
    return products
end
```

#### `execute(product)`

Called to build a single product. Receives a table with `inputs` and `outputs` keys (both tables of absolute path strings). Must create the output files on success or error on failure.

```lua
function execute(product)
    rsb.run_command("mytool", {product.inputs[1]})
    rsb.write_stub(product.outputs[1], "done")
end
```

### Optional Functions

#### `clean(product)`

Called when running `rsb clean`. Receives the same product table as `execute()`. Default behavior: removes all output files.

```lua
function clean(product)
    for _, output in ipairs(product.outputs) do
        rsb.remove_file(output)
    end
end
```

#### `auto_detect(files)`

Called to determine whether this processor is relevant for the project (when `auto_detect = true` in config). Receives the list of matching files. Default: returns `true` if the files list is non-empty.

```lua
function auto_detect(files)
    return #files > 0
end
```

#### `required_tools()`

Returns a table of external tool names required by this processor. Used by `rsb tools list` and `rsb tools check`. Default: empty table.

```lua
function required_tools()
    return {"eslint", "node"}
end
```

#### `hidden()`

Returns `true` to hide this processor from default `rsb processor list` output (still shown with `--all`). Default: `false`.

```lua
function hidden()
    return false
end
```

## The `rsb` Global Table

Lua plugins have access to an `rsb` global table with helper functions.

| Function | Description |
|---|---|
| `rsb.stub_path(project_root, source, suffix)` | Compute the stub output path for a source file. Maps `project_root/a/b/file.ext` to `out/suffix/a_b_file.ext.suffix`. |
| `rsb.run_command(program, args)` | Run an external command. Errors if the command fails (non-zero exit). |
| `rsb.run_command_cwd(program, args, cwd)` | Run an external command with a working directory. |
| `rsb.write_stub(path, content)` | Write a stub file (creates parent directories as needed). |
| `rsb.remove_file(path)` | Remove a file if it exists. No error if the file is missing. |
| `rsb.file_exists(path)` | Returns `true` if the file exists. |
| `rsb.read_file(path)` | Read a file and return its contents as a string. |
| `rsb.path_join(parts)` | Join path components. Takes a table: `rsb.path_join({"a", "b", "c"})` returns `"a/b/c"`. |
| `rsb.log(message)` | Print a message prefixed with the plugin name. |

## Configuration

Plugins use the standard scan configuration fields. Any `[processor.NAME]` section in `rsb.toml` is passed to the plugin's `discover()` function as the `config` table.

### Scan Configuration

These fields control which files are passed to `discover()`:

| Key | Type | Default | Description |
|---|---|---|---|
| `scan_dir` | string | `""` | Directory to scan (`""` = project root) |
| `extensions` | string[] | `[]` | File extensions to match |
| `exclude_dirs` | string[] | `[]` | Directory path segments to skip |
| `exclude_files` | string[] | `[]` | File names to skip |
| `exclude_paths` | string[] | `[]` | Paths relative to project root to skip |

### Custom Configuration

Any additional keys in the `[processor.NAME]` section are passed through to the Lua `config` table:

```toml
[processor.eslint]
scan_dir = "src"
extensions = [".js", ".ts"]
max_warnings = 0          # custom key, accessible as config.max_warnings in Lua
fix = false               # custom key, accessible as config.fix in Lua
```

```lua
function execute(product)
    local args = {product.inputs[1]}
    if config.max_warnings then
        table.insert(args, "--max-warnings")
        table.insert(args, tostring(config.max_warnings))
    end
    rsb.run_command("eslint", args)
    rsb.write_stub(product.outputs[1], "linted")
end
```

### Plugins Directory

The directory where RSB looks for `.lua` files is configurable:

```toml
[plugins]
dir = "plugins"  # default
```

## Plugin Name Resolution

The plugin name is derived from the `.lua` filename (without extension). This name is used for:

- The `[processor.NAME]` config section
- The `enabled` list in `[processor]`
- The `out/NAME/` stub directory
- Display in `rsb processor list` and build output

A plugin name must not conflict with a built-in processor name (`template`, `ruff`, `pylint`, `cc_single_file`, `cpplint`, `shellcheck`, `spellcheck`, `sleep`, `make`). RSB will error if a conflict is detected.

## Incremental Builds

Lua plugins participate in RSB's incremental build system automatically:

- Products are identified by their inputs, outputs, and a config hash
- If none of the declared inputs have changed since the last build, the product is skipped
- If the `[processor.NAME]` config section changes, all products are rebuilt
- Outputs are cached and can be restored from cache

For correct incrementality, make sure `discover()` declares all files that affect the output. If your tool reads additional configuration files, include them in the `inputs` list.

## Examples

### Stub-Based Linter

A typical linter plugin: run a tool on each file, create a stub on success.

```lua
function description()
    return "Lint YAML files with yamllint"
end

function required_tools()
    return {"yamllint"}
end

function discover(project_root, config, files)
    local products = {}
    for _, file in ipairs(files) do
        table.insert(products, {
            inputs = {file},
            outputs = {rsb.stub_path(project_root, file, "yamllint")},
        })
    end
    return products
end

function execute(product)
    rsb.run_command("yamllint", {"-s", product.inputs[1]})
    rsb.write_stub(product.outputs[1], "linted")
end
```

```toml
[processor]
enabled = ["yamllint"]

[processor.yamllint]
extensions = [".yml", ".yaml"]
```

### File Transformer

A plugin that transforms input files into output files (not stubs).

```lua
function description()
    return "Compile Sass to CSS"
end

function required_tools()
    return {"sass"}
end

function discover(project_root, config, files)
    local products = {}
    for _, file in ipairs(files) do
        local out = file:gsub("%.scss$", ".css"):gsub("^" .. project_root .. "/src/", project_root .. "/out/sass/")
        table.insert(products, {
            inputs = {file},
            outputs = {out},
        })
    end
    return products
end

function execute(product)
    rsb.run_command("sass", {product.inputs[1], product.outputs[1]})
end
```

```toml
[processor]
enabled = ["sass"]

[processor.sass]
scan_dir = "src"
extensions = [".scss"]
```
