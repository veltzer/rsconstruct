# RSBuild - Rust Build Tool

A fast, incremental build tool written in Rust with C/C++ compilation, template support, Python linting, and parallel execution.

## Documentation

Full documentation: <https://veltzer.github.io/rsbuild/>

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
gh release download latest --repo veltzer/rsbuild --pattern 'rsbuild-x86_64-unknown-linux-gnu' --output rsbuild --clobber

# aarch64 / arm64
gh release download latest --repo veltzer/rsbuild --pattern 'rsbuild-aarch64-unknown-linux-gnu' --output rsbuild --clobber

chmod +x rsbuild
sudo mv rsbuild /usr/local/bin/
```

Or without the GitHub CLI:

```bash
# x86_64
curl -Lo rsbuild https://github.com/veltzer/rsbuild/releases/download/latest/rsbuild-x86_64-unknown-linux-gnu

# aarch64 / arm64
curl -Lo rsbuild https://github.com/veltzer/rsbuild/releases/download/latest/rsbuild-aarch64-unknown-linux-gnu

chmod +x rsbuild
sudo mv rsbuild /usr/local/bin/
```

### Build from source

```bash
cargo build --release
```

## Quick Start

```bash
rsbuild init                     # Create a new project
rsbuild build                    # Incremental build
rsbuild build --force            # Force full rebuild
rsbuild build -j4                # Build with 4 parallel jobs
rsbuild build --timings          # Show timing info
rsbuild status                   # Show what needs rebuilding
rsbuild watch                    # Watch for changes and rebuild
rsbuild clean                    # Remove build artifacts
rsbuild graph --view             # Visualize dependency graph
rsbuild processor list           # List available processors
```
