# Mdl Processor

## Purpose

Lints Markdown files using [mdl](https://github.com/markdownlint/markdownlint) (Ruby markdownlint).

## How It Works

Discovers `.md` files in the project and runs `mdl` on each file. A non-zero
exit code fails the product.

With `local_repo = true`, mdl runs with `GEM_HOME`/`GEM_PATH` set to
`gem_home`, using the `mdl` gem that the [gem](../creator/gem.md) processor
installs, and lists the gem processor's install stamp (`gem_stamp`) as an
input of every product — so the graph runs mdl only after `bundle install`
succeeded, and a reinstall re-checks every file. Without `local_repo`, mdl
is the system install and does not depend on the gem processor.

## Source Files

- Input: `**/*.md`
- Output: none (checker)

## Configuration

```toml
[processor.checker.mdl]
local_repo = false                     # Run mdl from the gems the gem processor installs
gem_home = "gems"                      # GEM_HOME directory
command = "mdl"                        # Path to the mdl binary
args = []                              # Additional arguments to pass to mdl
gem_stamp = "out/processor.creator.gem/root.stamp"       # Gem install stamp (input when local_repo)
dep_inputs = []                      # Additional files that trigger rebuilds when changed
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `local_repo` | boolean | `false` | Run mdl from the local gem repository the gem processor installs, and depend on its install |
| `gem_home` | string | `"gems"` | GEM_HOME directory for Ruby gems (used when `local_repo`) |
| `command` | string | `"mdl"` | Path to the mdl executable |
| `args` | string[] | `[]` | Extra arguments passed to mdl |
| `gem_stamp` | string | `"out/processor.creator.gem/root.stamp"` | The gem processor's install stamp for the `Gemfile` mdl comes from; an input when `local_repo`, so mdl runs after the install. Point it at `out/processor.creator.gem/<dir>.stamp` for a `Gemfile` in a subdirectory |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

The tool processes one file at a time. Each file is checked in a separate invocation.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
