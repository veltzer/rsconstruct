# Gem Processor

## Purpose

Installs Ruby dependencies from `Gemfile` files using Bundler.

## How It Works

Discovers `Gemfile` files in the project and runs `bundle install` in each
directory with `GEM_HOME` and `GEM_PATH` set to `gem_home`, so gems install
into a directory next to the `Gemfile`. Sibling `.rb` and `.gemspec` files
are tracked as inputs. After a successful install it writes an install stamp,
`out/processor.creator.gem/root.stamp` for a root `Gemfile` (the directory
path with `/` turned into `_` otherwise). The stamp is what another
processor lists as an input to run after the install — the
[mdl](../checker/mdl.md) processor's `gem_stamp` does exactly that. With
`cache_output_dir = true` (the default) the `gem_home` directory is cached as
a tree together with the stamp, so `rsconstruct clean && rsconstruct build`
restores both instead of reinstalling; with it off, only the stamp is cached.

## Source Files

- Input: `**/Gemfile` (plus sibling `.rb`, `.gemspec` files)
- Output: the install stamp `out/processor.creator.gem/<dir>.stamp`, and the `gem_home` directory (`gems/` by default) next to each `Gemfile` when `cache_output_dir = true`

## Configuration

```toml
[processor.creator.gem]
command = "bundle"                     # The bundler command to run
args = []                              # Additional arguments to pass to bundler install
gem_home = "gems"                      # Where gems are installed (GEM_HOME/GEM_PATH)
dep_inputs = []                      # Additional files that trigger rebuilds when changed
cache_output_dir = true                # Cache the gem_home directory for fast restore after clean
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `command` | string | `"bundle"` | The bundler executable to run |
| `args` | string[] | `[]` | Extra arguments passed to bundler install |
| `gem_home` | string | `"gems"` | Directory, relative to the `Gemfile`, where gems are installed; exported as `GEM_HOME` and `GEM_PATH` |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |
| `cache_output_dir` | boolean | `true` | Cache the `gem_home` directory so `rsconstruct clean && rsconstruct build` restores from cache |

## Batch support

Runs as a single whole-project operation (e.g., `cargo build`, `npm install`).

## Clean behavior

This processor is a Creator — `rsconstruct clean outputs` removes its declared `output_dirs` recursively (the build tool produces an unknown set of files inside, so directory-level deletion is the only option). After all per-product cleans complete, the orchestrator removes any parent directories that are now empty. Pass `--no-empty-dirs` to keep them. See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
