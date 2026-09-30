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

### Download pre-built binary

Every release ships four binaries: `rsconstruct-linux-x86_64`,
`rsconstruct-linux-aarch64`, `rsconstruct-macos-x86_64` and
`rsconstruct-macos-aarch64`. Pick the one for your platform.

```bash
# Linux x86_64 (use rsconstruct-linux-aarch64, rsconstruct-macos-x86_64 or
# rsconstruct-macos-aarch64 for the other platforms)
gh release download --repo veltzer/rsconstruct --pattern 'rsconstruct-linux-x86_64' --output rsconstruct --clobber

chmod +x rsconstruct
sudo mv rsconstruct /usr/local/bin/
```

Or without the GitHub CLI:

```bash
# Linux x86_64 (substitute the asset name for the other platforms)
curl -Lo rsconstruct https://github.com/veltzer/rsconstruct/releases/latest/download/rsconstruct-linux-x86_64

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
rsconstruct processors list          # List available processors
```
