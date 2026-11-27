# Uv Processor

## Purpose

Installs a Python project's dependencies with
[uv](https://github.com/astral-sh/uv), a Rust tool: the counterpart of the
[pip](pip.md) processor for a project that has a `uv.lock`.

A project's own build environment needs no stanza for this: `rsconstruct tool
install project` already runs `uv sync` for the repository's `.venv` before
any build (see [Python packages live in the repo's
`.venv`](../../configuration.md#python-packages-live-in-the-repos-venv-owned-by-uv)).
This processor is for the environments that are build products themselves,
such as an example project in a subdirectory with its own `uv.lock`.

## How It Works

Discovers `uv.lock` files and runs `uv sync` in each one's directory, which
creates or updates the `.venv` beside it. After a successful sync it writes
an install stamp, `out/processor.mass_generator.uv/root.stamp` for a root
`uv.lock` (or `<dir>.stamp` with `/` turned into `_` otherwise). The lock and
the `pyproject.toml` beside it are the inputs, so the environment is synced
again only when one of them (or a `dep_inputs` file) changes.

The default `args` are `--locked`: a `pyproject.toml` that has moved on from
its `uv.lock` fails the build instead of being re-resolved during it. Run
`uv lock` and commit the result.

Under the sandbox the root `.venv` is writable because uv declares it. A
`uv.lock` in a subdirectory syncs `<dir>/.venv`, which the stanza must
declare: `sandbox_writable = ["<dir>/.venv"]`.

## Source Files

- Input: `**/uv.lock` and the `pyproject.toml` beside each
- Output: the install stamp `out/processor.mass_generator.uv/<dir>.stamp`

## Configuration

```toml
[processor.mass_generator.uv]
src_dirs = ["."]
args = ["--locked"]                    # The default
```

| Key | Type | Default | Description |
| ----- | ------ | --------- | ------------- |
| `command` | string | `"uv"` | The uv executable to run |
| `args` | string[] | `["--locked"]` | Arguments passed after `uv sync` |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |
| `required_tools` | string[] | `[]` | Extra tools the build needs on PATH beyond the built-in ones (e.g. what a wrapper script shells out to); checked before building and covered by `tool install` and version locking |

## Batch support

Runs once per `uv.lock` when its install stamp product is dirty.

## Clean behavior

This processor is a Mass Generator — `rsconstruct clean outputs` removes each planned install stamp (`product.outputs`) and sweeps any parent directories that become empty. The `.venv` is not an output and is left in place. See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
