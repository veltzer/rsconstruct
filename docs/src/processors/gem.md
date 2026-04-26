# Gem Processor

## Purpose

Installs Ruby dependencies from `Gemfile` files using Bundler.

## How It Works

Discovers `Gemfile` files in the project, runs `bundle install` in each
directory, and creates a stamp file on success. Sibling `.rb` and `.gemspec`
files are tracked as inputs.

## Source Files

- Input: `**/Gemfile` (plus sibling `.rb`, `.gemspec` files)
- Output: `out/gem/{flat_name}.stamp`

## Configuration

```toml
[processor.gem]
command = "bundle"                     # The bundler command to run
args = []                              # Additional arguments to pass to bundler install
dep_inputs = []                      # Additional files that trigger rebuilds when changed
cache_output_dir = true                # Cache the vendor/bundle directory for fast restore after clean
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `command` | string | `"bundle"` | The bundler executable to run |
| `args` | string[] | `[]` | Extra arguments passed to bundler install |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |
| `cache_output_dir` | boolean | `true` | Cache the `vendor/bundle/` directory so `rsconstruct clean && rsconstruct build` restores from cache |

## Batch support

Runs as a single whole-project operation (e.g., `cargo build`, `npm install`).

## Clean behavior

This processor is a Creator — `rsconstruct clean outputs` removes its declared `output_dirs` recursively (the build tool produces an unknown set of files inside, so directory-level deletion is the only option). After all per-product cleans complete, the orchestrator removes any parent directories that are now empty. Pass `--no-empty-dirs` to keep them. See [Clean behavior](../processors.md#clean-behavior) and [`rsconstruct clean`](../commands.md#rsconstruct-clean).
