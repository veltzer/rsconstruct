# RSB Design Notes

RSB is a Rust build tool with incremental builds using SHA-256 checksums.

## Core Commands

- `rsb build` — incremental build (only rebuilds changed files)
- `rsb clean` — remove build artifacts (preserves cache)
- `rsb distclean` — remove all build and cache directories (.rsb/ and out/)
- `rsb status` — show product status (up-to-date, stale, or restorable)
- `rsb init` — initialize a new rsb project
- `rsb watch` — watch source files and auto-rebuild on changes
- `rsb graph` — display the build dependency graph
- `rsb cache` — manage the build cache (clear, size, trim, list)
- `rsb processor` — manage processors (list)
- `rsb complete` — generate shell completions
- `rsb version` — print version information

## CLI

Built with clap (derive API) for command line parsing with shell completion support.

## Config System

Configuration is in `rsb.toml`. Python config files live in the `config/` folder by convention.

The `load_python` function in Tera loads Python config files from any path and makes
the config values available for templating. The config files are usually in a folder
`config/` beside `templates/`.

## Processors

Processors implement the `ProductDiscovery` trait. Each processor discovers products
(input/output pairs), and the executor builds them in dependency order.

Available processors:
- **template** — Tera template processing (`templates/{X}.tera` → `{X}`)
- **cc_single_file** — C/C++ single-file compilation with automatic header dependency tracking
- **pylint** — Python linting with ruff (configurable)
- **cpplint** — C/C++ static analysis with cppcheck (configurable)
- **sleep** — sleep for testing parallel execution

## Caching

Products are cached using SHA-256 checksums of inputs. Processor configuration
(compiler flags, linter args, etc.) is hashed into the cache key so that config
changes trigger rebuilds without requiring `--force`.

Cache can be restored via hardlinks (default, fast) or copies (cross-filesystem safe).

## Templates

Convention over configuration: every file in `templates/{X}.tera` creates a file
called `{X}` (no templates prefix and no .tera suffix) using the Tera templating engine.
