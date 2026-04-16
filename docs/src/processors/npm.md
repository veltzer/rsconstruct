# Npm Processor

## Purpose

Installs Node.js dependencies from `package.json` files using npm.

## How It Works

Discovers `package.json` files in the project, runs `npm install` in each
directory, and creates a stamp file on success. Sibling `.json`, `.js`, and
`.ts` files are tracked as inputs so changes trigger reinstallation.

## Source Files

- Input: `**/package.json` (plus sibling `.json`, `.js`, `.ts` files)
- Output: `out/npm/{flat_name}.stamp`

## Configuration

```toml
[processor.npm]
command = "npm"                        # The npm command to run
args = []                              # Additional arguments to pass to npm install
dep_inputs = []                      # Additional files that trigger rebuilds when changed
cache_output_dir = true                # Cache the node_modules directory for fast restore after clean
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `command` | string | `"npm"` | The npm executable to run |
| `args` | string[] | `[]` | Extra arguments passed to npm install |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |
| `cache_output_dir` | boolean | `true` | Cache the `node_modules/` directory so `rsconstruct clean && rsconstruct build` restores from cache |

## Batch support

Runs as a single whole-project operation (e.g., `cargo build`, `npm install`).
