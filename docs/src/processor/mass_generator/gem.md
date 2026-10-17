# Gem Processor

## Purpose

Installs Ruby dependencies from `Gemfile` files using Bundler.

## How It Works

Discovers `Gemfile` files in the project and runs `bundle install` in each
directory with `GEM_HOME` and `GEM_PATH` set to `gem_home`, so gems install
into a directory next to the `Gemfile`. Sibling `.rb` and `.gemspec` files
are tracked as inputs. After a successful install it writes an install stamp,
`out/processor.mass_generator.gem/root.stamp` for a root `Gemfile` (the directory
path with `/` turned into `_` otherwise). The stamp is what another
processor lists as an input to run after the install — the
[mdl](../checker/mdl.md) processor's `gem_stamp` does exactly that.

## Source Files

- Input: `**/Gemfile` (plus sibling `.rb`, `.gemspec` files)
- Output: the install stamp `out/processor.mass_generator.gem/<dir>.stamp`

## Configuration

```toml
[processor.mass_generator.gem]
command = "bundle"                     # The bundler command to run
args = []                              # Additional arguments to pass to bundler install
gem_home = "gems"                      # Where gems are installed (GEM_HOME/GEM_PATH)
dep_inputs = []                        # Additional files that trigger rebuilds when changed
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `command` | string | `"bundle"` | The bundler executable to run |
| `args` | string[] | `[]` | Extra arguments passed to bundler install |
| `gem_home` | string | `"gems"` | Directory, relative to the `Gemfile`, where gems are installed; exported as `GEM_HOME` and `GEM_PATH` |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

Runs once per `Gemfile` when its install stamp product is dirty.

## Clean behavior

This processor is a Mass Generator — `rsconstruct clean outputs` removes each planned install stamp (`product.outputs`) and sweeps any parent directories that become empty. See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
