# Pytest Processor

## Purpose

Runs Python test files using pytest to verify they pass.

## How It Works

Python test files (`.py`) in the `tests/` directory are run using `pytest`.
Each test file is checked individually — a failing test causes the build to fail.

## Source Files

- Input: `tests/**/*.py`
- Output: none (checker — pass/fail only)

## Configuration

```toml
[processor.pytest]
extensions = [".py"]                      # File extensions to process (default: [".py"])
scan_dirs = ["tests"]                     # Directories to scan (default: ["tests"])
extra_inputs = []                         # Additional files that trigger rebuilds
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `extensions` | string[] | `[".py"]` | File extensions to discover |
| `scan_dirs` | string[] | `["tests"]` | Directories to scan for test files |
| `extra_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |
