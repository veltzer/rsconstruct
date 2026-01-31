# RSB Design Notes

RSB is a Rust build tool with incremental builds using SHA-256 checksums.

## Core Commands

- `rsb build` — incremental build (only rebuilds changed files)
- `rsb clean` — remove build artifacts (default: outputs only, preserves cache)
- `rsb clean all` — remove all build and cache directories (.rsb/ and out/)
- `rsb clean git` — hard clean using git clean (requires git repository)
- `rsb status` — show product status (up-to-date, stale, or restorable)
- `rsb init` — initialize a new rsb project
- `rsb watch` — watch source files and auto-rebuild on changes
- `rsb graph` — display the build dependency graph
- `rsb cache` — manage the build cache (clear, size, trim, list)
- `rsb config` — show active or default configuration
- `rsb processor` — manage processors (list, all, auto, files)
- `rsb tools` — list or check required external tools
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
- **template** — Tera template processing (`templates/{X}.tera` -> `{X}`)
- **ruff** — Python linting with ruff (configurable linter binary)
- **pylint** — Python linting with pylint
- **cc_single_file** — C/C++ single-file compilation with automatic header dependency tracking
- **cpplint** — C/C++ static analysis with cppcheck (configurable)
- **spellcheck** — documentation spell checking using hunspell dictionaries
- **sleep** — sleep for testing parallel execution
- **make** — run make in directories containing Makefiles

## File Indexing

All file discovery is done through a single `FileIndex` built once per invocation.
The index uses the `ignore` crate (`ignore::WalkBuilder`) which natively handles:

- `.gitignore` — standard git ignore rules, including nested `.gitignore` files
- `.rsbignore` — project-specific ignore patterns (same glob syntax as `.gitignore`)

Processors query the pre-built index with their scan configuration (extensions,
exclude directories, exclude files) instead of walking the filesystem individually.

## Caching

Products are cached using SHA-256 checksums of inputs. Processor configuration
(compiler flags, linter args, etc.) is hashed into the cache key so that config
changes trigger rebuilds without requiring `--force`.

Cache can be restored via hardlinks (default, fast) or copies (cross-filesystem safe).

## Templates

Convention over configuration: every file in `templates/{X}.tera` creates a file
called `{X}` (no templates prefix and no .tera suffix) using the Tera templating engine.
