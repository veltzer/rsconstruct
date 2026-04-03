# Jekyll Processor

## Purpose

Builds Jekyll static sites by running `jekyll build` in directories containing
a `_config.yml` file.

## How It Works

Discovers `_config.yml` files in the project (excluding common build tool
directories). For each one, runs `jekyll build` in that directory.

## Source Files

- Input: `**/_config.yml`
- Output: none (mass generator)

## Configuration

```toml
[processor.jekyll]
args = []
extra_inputs = []
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `args` | string[] | `[]` | Extra arguments passed to jekyll build |
| `extra_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

Runs as a single whole-project operation (e.g., `cargo build`, `npm install`).
