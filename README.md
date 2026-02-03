# RSB - Rust Build Tool

A fast, incremental build tool written in Rust with C/C++ compilation, template support, Python linting, and parallel execution.

## Documentation

Full documentation: <https://veltzer.github.io/rsb/>

## Features

- **Incremental builds** using SHA-256 checksums to detect changes
- **C/C++ compilation** with automatic header dependency tracking
- **Parallel execution** of independent build products with `-j` flag
- **Template processing** via the Tera templating engine
- **Python linting** with ruff (configurable)
- **Deterministic builds** — same input always produces same build order
- **Graceful interrupt** — Ctrl+C saves progress, next build resumes where it left off
- **Config-aware caching** — changing compiler flags or linter config triggers rebuilds
- **Convention over configuration** — simple naming conventions, minimal config needed

## Installation

### Download pre-built binary (Linux)

Pre-built binaries are available for x86_64 and aarch64 (arm64).

```bash
# x86_64
gh release download latest --repo veltzer/rsb --pattern 'rsb-x86_64-unknown-linux-gnu' --output rsb --clobber

# aarch64 / arm64
gh release download latest --repo veltzer/rsb --pattern 'rsb-aarch64-unknown-linux-gnu' --output rsb --clobber

chmod +x rsb
sudo mv rsb /usr/local/bin/
```

Or without the GitHub CLI:

```bash
# x86_64
curl -Lo rsb https://github.com/veltzer/rsb/releases/download/latest/rsb-x86_64-unknown-linux-gnu

# aarch64 / arm64
curl -Lo rsb https://github.com/veltzer/rsb/releases/download/latest/rsb-aarch64-unknown-linux-gnu

chmod +x rsb
sudo mv rsb /usr/local/bin/
```

### Build from source

```bash
cargo build --release
```

## Quick Start

```bash
rsb init                     # Create a new project
rsb build                    # Incremental build
rsb build --force            # Force full rebuild
rsb build -j4                # Build with 4 parallel jobs
rsb build --timings          # Show timing info
rsb status                   # Show what needs rebuilding
rsb watch                    # Watch for changes and rebuild
rsb clean                    # Remove build artifacts
rsb graph --view             # Visualize dependency graph
rsb processor list           # List available processors
```
