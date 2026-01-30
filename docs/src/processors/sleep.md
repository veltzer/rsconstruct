# Sleep Processor

## Purpose

Sleeps for a configured duration. Used for testing parallel execution and
build infrastructure; not intended for production use.

## How It Works

Discovers `.sleep` files in the `sleep/` directory. Each file contains a
floating-point number representing seconds to sleep. The processor sleeps
for the specified duration and creates a stub file recording the elapsed time.

## Source Files

- Input: `sleep/**/*.sleep`
- Output: `out/sleep/{basename}.done`

## Configuration

```toml
[processor.sleep]
extra_inputs = []    # Additional files that trigger rebuilds when changed
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `extra_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |
