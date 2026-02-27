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
bundler = "bundle"                     # The bundler command to run
command = "install"                    # The bundle subcommand to execute
args = []                              # Additional arguments to pass to bundler
extra_inputs = []                      # Additional files that trigger rebuilds when changed
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `bundler` | string | `"bundle"` | The bundler executable to run |
| `command` | string | `"install"` | The bundle subcommand to execute |
| `args` | string[] | `[]` | Extra arguments passed to bundler |
| `extra_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |
