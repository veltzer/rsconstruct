# Pip Processor

## Purpose

Installs Python dependencies from `requirements.txt` files using pip.

## How It Works

Discovers `requirements.txt` files in the project and runs `pip install -r`
in each file's directory. After a successful install it writes a deterministic
install stamp, `out/processor.mass_generator.pip/root.stamp` for a root
`requirements.txt` (or `<dir>.stamp` with `/` turned into `_` otherwise), so
dependencies are only reinstalled when `requirements.txt` (or a `dep_inputs`
file) changes.

## Source Files

- Input: `**/requirements.txt`
- Output: the install stamp `out/processor.mass_generator.pip/<dir>.stamp`

## Configuration

```toml
[processor.mass_generator.pip]
command = "pip"                        # The pip command to run
args = []                              # Additional arguments to pass to pip
dep_inputs = []                        # Additional files that trigger rebuilds when changed
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `command` | string | `"pip"` | The pip executable to run |
| `args` | string[] | `[]` | Extra arguments passed to pip |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

Runs once per `requirements.txt` when its install stamp product is dirty.

## Clean behavior

This processor is a Mass Generator — `rsconstruct clean outputs` removes each planned install stamp (`product.outputs`) and sweeps any parent directories that become empty. See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
