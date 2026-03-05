# Pip Processor

## Purpose

Installs Python dependencies from `requirements.txt` files using pip.

## How It Works

Discovers `requirements.txt` files in the project, runs `pip install -r` on
each, and creates a stamp file on success. The stamp file tracks the install
state so dependencies are only reinstalled when `requirements.txt` changes.

## Source Files

- Input: `**/requirements.txt`
- Output: `out/pip/{flat_name}.stamp`

## Configuration

```toml
[processor.pip]
pip = "pip"                            # The pip command to run
args = []                              # Additional arguments to pass to pip
extra_inputs = []                      # Additional files that trigger rebuilds when changed
cache_output_dir = true                # Cache the stamp directory for fast restore after clean
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `pip` | string | `"pip"` | The pip executable to run |
| `args` | string[] | `[]` | Extra arguments passed to pip |
| `extra_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |
| `cache_output_dir` | boolean | `true` | Cache the `out/pip/` directory so `rsbuild clean && rsbuild build` restores from cache |
