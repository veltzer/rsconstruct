# Mdbook Processor

## Purpose

Builds [mdbook](https://rust-lang.github.io/mdBook/) documentation projects, planning every HTML page, theme asset, font, and static asset natively in Rust and caching each file as a product of its own.

## How It Works

Discovers `book.toml` files indicating mdbook projects, parses `book.toml` and `SUMMARY.md` along with static assets to compute the output plan under `output_dir`, and runs `mdbook build` with `MDBOOK_OUTPUT__HTML__HASH_FILES=false` into a staging directory when any planned product is dirty. Staged outputs are verified against the plan via `install_staged`.

Every planned file depends on `book.toml`, every file under the source
directory, the `additional-css`/`additional-js` files, and every file of a
custom theme — `theme/` next to `book.toml`, or the directory `[output.html]
theme` names — since a template, stylesheet or font there can change every
page. A book with no theme directory has no such inputs.

## Source Files

- Input: `**/book.toml` (plus `SUMMARY.md`, chapter `.md` files, static assets, and the custom theme directory)
- Output: Planned HTML pages, theme/search/font assets, and copied static files under `book` (or configured `output_dir`)

## Configuration

```toml
[processor.mass_generator.mdbook]
command = "mdbook"                     # The mdbook command to run
output_dir = "book"                    # Output directory for generated docs
loose_manifest = false                 # Warn instead of failing on plan mismatch
args = []                              # Additional arguments to pass to mdbook
dep_inputs = []                        # Additional files that trigger rebuilds when changed
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `command` | string | `"mdbook"` | The mdbook executable to run |
| `output_dir` | string | `"book"` | Output directory for generated documentation (relative to `book.toml` unless `build.build-dir` is set in `book.toml`) |
| `loose_manifest` | boolean | `false` | Warn instead of failing when `mdbook build` writes files not in the plan |
| `args` | string[] | `[]` | Extra arguments passed to mdbook |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |
| `required_tools` | string[] | `[]` | Extra tools the build needs on PATH beyond the built-in ones (e.g. what a wrapper script shells out to); checked before building and covered by `tool install` and version locking |

## Batch support

Runs once per `book.toml` when any of its planned output files is dirty.

## Clean behavior

This processor is a Mass Generator — `rsconstruct clean outputs` removes each planned output file (`product.outputs`) and sweeps any parent directories that are now empty. Pass `--no-empty-dirs` to keep them. See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
