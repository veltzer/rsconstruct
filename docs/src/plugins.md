# Lua Plugins

RSBuild supports custom processors written in Lua. Drop a `.lua` file in the `plugins/` directory and add its name to `[processor] enabled` in `rsbuild.toml`. The plugin participates in discovery, execution, caching, cleaning, tool listing, and auto-detection just like a built-in processor.

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
        local stub = rsbuild.stub_path(project_root, file, "eslint")
        table.insert(products, {
            inputs = {file},
            outputs = {stub},
        })
    end
    return products
end

function execute(product)
    rsbuild.run_command("eslint", {product.inputs[1]})
    rsbuild.write_stub(product.outputs[1], "linted")
end
```

**2. Enable it in `rsbuild.toml`:**

```toml
[processor]
enabled = ["eslint"]

[processor.eslint]
scan_dir = "src"
extensions = [".js", ".ts"]
```

**3. Run it:**

```sh
rsbuild build            # builds including the plugin
rsbuild processors list   # shows the plugin
rsbuild processors files  # shows files discovered by the plugin
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
        local stub = rsbuild.stub_path(project_root, file, "myplugin")
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
    rsbuild.run_command("mytool", {product.inputs[1]})
    rsbuild.write_stub(product.outputs[1], "done")
end
```

### Optional Functions

#### `clean(product)`

Called when running `rsbuild clean`. Receives the same product table as `execute()`. Default behavior: removes all output files.

```lua
function clean(product)
    for _, output in ipairs(product.outputs) do
        rsbuild.remove_file(output)
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

Returns a table of external tool names required by this processor. Used by `rsbuild tools list` and `rsbuild tools check`. Default: empty table.

```lua
function required_tools()
    return {"eslint", "node"}
end
```

#### `hidden()`

Returns `true` to hide this processor from default `rsbuild processors list` output (still shown with `--all`). Default: `false`.

```lua
function hidden()
    return false
end
```

#### `processor_type()`

Returns the type of processor: `"generator"` or `"checker"`. Generators create real output files (e.g., compilers, transpilers). Checkers validate input files; for checkers, you can choose whether to produce stub files or not. Default: `"checker"`.

**Option 1: Checker with stub files (for Lua plugins)**
```lua
function processor_type()
    return "checker"
end
```
When using stub files, return `outputs = {stub}` from `discover()` and call `rsbuild.write_stub()` in `execute()`.

**Option 2: Checker without stub files**
```lua
function processor_type()
    return "checker"
end
```
Return `outputs = {}` from `discover()` and don't write stubs in `execute()`. The cache database entry itself serves as the success record.

## The `rsbuild` Global Table

Lua plugins have access to an `rsbuild` global table with helper functions.

| Function | Description |
|---|---|
| `rsbuild.stub_path(project_root, source, suffix)` | Compute the stub output path for a source file. Maps `project_root/a/b/file.ext` to `out/suffix/a_b_file.ext.suffix`. |
| `rsbuild.run_command(program, args)` | Run an external command. Errors if the command fails (non-zero exit). |
| `rsbuild.run_command_cwd(program, args, cwd)` | Run an external command with a working directory. |
| `rsbuild.write_stub(path, content)` | Write a stub file (creates parent directories as needed). |
| `rsbuild.remove_file(path)` | Remove a file if it exists. No error if the file is missing. |
| `rsbuild.file_exists(path)` | Returns `true` if the file exists. |
| `rsbuild.read_file(path)` | Read a file and return its contents as a string. |
| `rsbuild.path_join(parts)` | Join path components. Takes a table: `rsbuild.path_join({"a", "b", "c"})` returns `"a/b/c"`. |
| `rsbuild.log(message)` | Print a message prefixed with the plugin name. |

## Configuration

Plugins use the standard scan configuration fields. Any `[processor.NAME]` section in `rsbuild.toml` is passed to the plugin's `discover()` function as the `config` table.

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
    rsbuild.run_command("eslint", args)
    rsbuild.write_stub(product.outputs[1], "linted")
end
```

### Plugins Directory

The directory where RSBuild looks for `.lua` files is configurable:

```toml
[plugins]
dir = "plugins"  # default
```

## Plugin Name Resolution

The plugin name is derived from the `.lua` filename (without extension). This name is used for:

- The `[processor.NAME]` config section
- The `enabled` list in `[processor]`
- The `out/NAME/` stub directory
- Display in `rsbuild processors list` and build output

A plugin name must not conflict with a built-in processor name (`tera`, `ruff`, `pylint`, `cc_single_file`, `cppcheck`, `shellcheck`, `spellcheck`, `sleep`, `make`). RSBuild will error if a conflict is detected.

## Incremental Builds

Lua plugins participate in RSBuild's incremental build system automatically:

- Products are identified by their inputs, outputs, and a config hash
- If none of the declared inputs have changed since the last build, the product is skipped
- If the `[processor.NAME]` config section changes, all products are rebuilt
- Outputs are cached and can be restored from cache

For correct incrementality, make sure `discover()` declares all files that affect the output. If your tool reads additional configuration files, include them in the `inputs` list.

## Examples

### Linter Without Stub Files (Recommended)

A checker that validates files without producing stub files. Success is recorded in the cache database.

```lua
function description()
    return "Lint YAML files with yamllint"
end

function processor_type()
    return "checker"
end

function required_tools()
    return {"yamllint"}
end

function discover(project_root, config, files)
    local products = {}
    for _, file in ipairs(files) do
        table.insert(products, {
            inputs = {file},
            outputs = {},  -- No output files
        })
    end
    return products
end

function execute(product)
    rsbuild.run_command("yamllint", {"-s", product.inputs[1]})
    -- No stub to write; cache entry = success
end

function clean(product)
    -- Nothing to clean
end
```

```toml
[processor]
enabled = ["yamllint"]

[processor.yamllint]
extensions = [".yml", ".yaml"]
```

### Stub-Based Linter (Legacy)

A linter that creates stub files. Use this if you need the stub file for some reason.

```lua
function description()
    return "Lint YAML files with yamllint"
end

function processor_type()
    return "checker"
end

function required_tools()
    return {"yamllint"}
end

function discover(project_root, config, files)
    local products = {}
    for _, file in ipairs(files) do
        table.insert(products, {
            inputs = {file},
            outputs = {rsbuild.stub_path(project_root, file, "yamllint")},
        })
    end
    return products
end

function execute(product)
    rsbuild.run_command("yamllint", {"-s", product.inputs[1]})
    rsbuild.write_stub(product.outputs[1], "linted")
end
```

```toml
[processor]
enabled = ["yamllint"]

[processor.yamllint]
extensions = [".yml", ".yaml"]
```

### File Transformer (Generator)

A plugin that transforms input files into output files (not stubs). This is a "generator" processor.

```lua
function description()
    return "Compile Sass to CSS"
end

function processor_type()
    return "generator"
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
    rsbuild.run_command("sass", {product.inputs[1], product.outputs[1]})
end
```

```toml
[processor]
enabled = ["sass"]

[processor.sass]
scan_dir = "src"
extensions = [".scss"]
```
