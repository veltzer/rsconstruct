# Jekyll Processor

## Purpose

Builds Jekyll static sites by planning the site outputs via Jekyll's own read/generate phase and running `jekyll build` into a staging directory.

## How It Works

Discovers `_config.yml` files in the project (excluding common build tool
directories). During discovery, runs an embedded Ruby planner that loads
`Jekyll::Site`, runs `reset`, `read`, and `generate` (without rendering), and
emits the planned destination paths. Each planned file becomes a cached product
under `output_dir` (`_site` by default, relative to `_config.yml`). When any
product is dirty, runs `jekyll build --disable-disk-cache -d <staging>` once
and verifies the staged outputs against the plan via `install_staged`.

## Source Files

- Input: `**/_config.yml` (plus sibling site files)
- Output: Planned site files under `_site/` (or configured `output_dir`)

## Configuration

```toml
[processor.mass_generator.jekyll]
command = "jekyll"
ruby = "ruby"
output_dir = "_site"
loose_manifest = false
args = []
dep_inputs = []
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `command` | string | `"jekyll"` | Path to the jekyll executable |
| `ruby` | string | `"ruby"` | Ruby interpreter for the discovery-time planner (must have the `jekyll` gem) |
| `output_dir` | string | `"_site"` | Output directory for generated site files, relative to `_config.yml` |
| `loose_manifest` | bool | `false` | Warn instead of failing when `jekyll build` writes files not in the plan |
| `args` | string[] | `[]` | Extra arguments passed to `jekyll build` |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

Runs once per `_config.yml` site when any of its planned output files is dirty.

## Clean behavior

This processor is a Mass Generator — `rsconstruct clean outputs` removes each planned output file (`product.outputs`) and sweeps any parent directories that are now empty. Pass `--no-empty-dirs` to keep them. See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
