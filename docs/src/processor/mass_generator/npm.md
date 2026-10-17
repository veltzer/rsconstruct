# Npm Processor

## Purpose

Installs Node.js dependencies from `package.json` files using npm.

## How It Works

Discovers `package.json` files in the project and runs `npm install` in each
directory. Sibling `.json`, `.js`, and `.ts` files are tracked as inputs so
changes trigger reinstallation. After a successful install it writes a
deterministic install stamp, `out/processor.mass_generator.npm/root.stamp` for a
root `package.json` (or `<dir>.stamp` with `/` turned into `_` otherwise).

## Source Files

- Input: `**/package.json` (plus sibling `.json`, `.js`, `.ts` files)
- Output: the install stamp `out/processor.mass_generator.npm/<dir>.stamp`

## Configuration

```toml
[processor.mass_generator.npm]
command = "npm"                        # The npm command to run
args = []                              # Additional arguments to pass to npm install
dep_inputs = []                        # Additional files that trigger rebuilds when changed
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `command` | string | `"npm"` | The npm executable to run |
| `args` | string[] | `[]` | Extra arguments passed to npm install |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

Runs once per `package.json` when its install stamp product is dirty.

## Clean behavior

This processor is a Mass Generator — `rsconstruct clean outputs` removes each planned install stamp (`product.outputs`) and sweeps any parent directories that become empty. See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
