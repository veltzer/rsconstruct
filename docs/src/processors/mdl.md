# Mdl Processor

## Purpose

Lints Markdown files using [mdl](https://github.com/markdownlint/markdownlint) (Ruby markdownlint).

## How It Works

Discovers `.md` files in the project and runs `mdl` on each file. A non-zero
exit code fails the product.

Depends on the gem processor — uses the `mdl` binary installed by Bundler.

## Source Files

- Input: `**/*.md`
- Output: none (checker)

## Configuration

```toml
[processor.mdl]
gem_home = "gems"                      # GEM_HOME directory
mdl_bin = "gems/bin/mdl"              # Path to the mdl binary
args = []                              # Additional arguments to pass to mdl
gem_stamp = "out/gem/root.stamp"       # Stamp file from gem processor (dependency)
extra_inputs = []                      # Additional files that trigger rebuilds when changed
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `gem_home` | string | `"gems"` | GEM_HOME directory for Ruby gems |
| `mdl_bin` | string | `"gems/bin/mdl"` | Path to the mdl executable |
| `args` | string[] | `[]` | Extra arguments passed to mdl |
| `gem_stamp` | string | `"out/gem/root.stamp"` | Stamp file from gem processor (ensures gems are installed first) |
| `extra_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |
