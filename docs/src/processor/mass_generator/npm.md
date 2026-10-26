# Npm Processor

## Purpose

Installs Node.js dependencies from `package.json` files using npm.

## How It Works

Discovers `package.json` files in the project and runs `npm install` in each
directory. The inputs are the files `npm install` reads: `package.json`,
and whichever of `package-lock.json`, `npm-shrinkwrap.json` and `.npmrc`
sit next to it. The package's own `.js`/`.ts` sources and nested packages
are not inputs — editing them changes nothing about what gets installed, so
it must not re-run the install. After a successful install it writes a
deterministic install stamp, `out/processor.mass_generator.npm/root.stamp` for a
root `package.json` (or `<dir>.stamp` with `/` turned into `_` otherwise).

## Source Files

- Input: `**/package.json` (plus the sibling `package-lock.json`, `npm-shrinkwrap.json`, `.npmrc`)
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
| `required_tools` | string[] | `[]` | Extra tools the build needs on PATH beyond the built-in ones (e.g. what a wrapper script shells out to); checked before building and covered by `tool install` and version locking |

## Batch support

Runs once per `package.json` when its install stamp product is dirty.

## Clean behavior

This processor is a Mass Generator — `rsconstruct clean outputs` removes each planned install stamp (`product.outputs`) and sweeps any parent directories that become empty. See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
