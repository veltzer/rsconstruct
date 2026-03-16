# RSConstruct - Rust Build Tool

A fast, incremental build tool written in Rust with C/C++ compilation, template support, Python linting, and parallel execution.

## Documentation

Full documentation: <https://veltzer.github.io/rsconstruct/>

## Features

- **Incremental builds** using SHA-256 checksums to detect changes
- **Remote caching** — share build artifacts across machines via S3, HTTP, or filesystem
- **C/C++ compilation** with automatic header dependency tracking
- **Parallel execution** of independent build products with `-j` flag
- **Template processing** via the Tera templating engine
- **Python linting** with ruff (configurable)
- **Lua plugins** — extend with custom processors without forking
- **Deterministic builds** — same input always produces same build order
- **Graceful interrupt** — Ctrl+C saves progress, next build resumes where it left off
- **Config-aware caching** — changing compiler flags or linter config triggers rebuilds
- **Convention over configuration** — simple naming conventions, minimal config needed

## Installation

### Download pre-built binary (Linux)

Pre-built binaries are available for x86_64 and aarch64 (arm64).

```bash
# x86_64
gh release download latest --repo veltzer/rsconstruct --pattern 'rsconstruct-x86_64-unknown-linux-gnu' --output rsconstruct --clobber

# aarch64 / arm64
gh release download latest --repo veltzer/rsconstruct --pattern 'rsconstruct-aarch64-unknown-linux-gnu' --output rsconstruct --clobber

chmod +x rsconstruct
sudo mv rsconstruct /usr/local/bin/
```

Or without the GitHub CLI:

```bash
# x86_64
curl -Lo rsconstruct https://github.com/veltzer/rsconstruct/releases/download/latest/rsconstruct-x86_64-unknown-linux-gnu

# aarch64 / arm64
curl -Lo rsconstruct https://github.com/veltzer/rsconstruct/releases/download/latest/rsconstruct-aarch64-unknown-linux-gnu

chmod +x rsconstruct
sudo mv rsconstruct /usr/local/bin/
```

### Build from source

```bash
cargo build --release
```

## Quick Start

```bash
rsconstruct init                     # Create a new project
rsconstruct build                    # Incremental build
rsconstruct build --force            # Force full rebuild
rsconstruct build -j4                # Build with 4 parallel jobs
rsconstruct build --timings          # Show timing info
rsconstruct status                   # Show what needs rebuilding
rsconstruct watch                    # Watch for changes and rebuild
rsconstruct clean                    # Remove build artifacts
rsconstruct graph --view             # Visualize dependency graph
rsconstruct processor list           # List available processors
```
