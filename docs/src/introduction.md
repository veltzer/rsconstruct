# RSBuild - Rust Build Tool

I added this

A fast, incremental build tool written in Rust with C/C++ compilation, template support, Python linting, and parallel execution.

## Features

- **Incremental builds** using SHA-256 checksums to detect changes
- **C/C++ compilation** with automatic header dependency tracking
- **Parallel execution** of independent build products with `-j` flag
- **Template processing** via the Tera templating engine
- **Python linting** with ruff and pylint
- **Documentation spell checking** using hunspell dictionaries
- **Make integration** — run make in directories containing Makefiles
- **`.gitignore` support** — respects `.gitignore` and `.rsbuildignore` patterns
- **Deterministic builds** — same input always produces same build order
- **Graceful interrupt** — Ctrl+C saves progress, next build resumes where it left off
- **Config-aware caching** — changing compiler flags or linter config triggers rebuilds
- **Convention over configuration** — simple naming conventions, minimal config needed

## Philosophy

Convention over configuration — simple naming conventions, explicit config loading, incremental builds by default.
