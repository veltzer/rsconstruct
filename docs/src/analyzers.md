# Dependency Analyzers

rsconstruct uses **dependency analyzers** to scan source files and discover dependencies between files. Analyzers run after processors discover products and add dependency information to the build graph.

## How analyzers work

1. **Product discovery**: Processors discover products (source → output mappings).
2. **Dependency analysis**: Analyzers scan source files to find dependencies.
3. **Graph resolution**: Dependencies are added to products for correct build ordering.

Analyzers are decoupled from processors — they operate on any product with matching source files, regardless of which processor created it.

## Built-in analyzers

Per-analyzer reference pages:

- [cpp](analyzers/cpp.md) — C/C++ `#include` scanning (invokes `gcc`/`pkg-config`)
- [icpp](analyzers/icpp.md) — C/C++ `#include` scanning, pure Rust (no subprocess)
- [python](analyzers/python.md) — Python `import` / `from ... import` resolution
- [markdown](analyzers/markdown.md) — Markdown image and link references
- [sass](analyzers/sass.md) — Sass/SCSS `@use`, `@forward` and `@import` resolution
- [tera](analyzers/tera.md) — Tera `{% include %}`, `{% import %}`, `{% extends %}` references

## Configuration

Analyzers are configured in `rsconstruct.toml`:

```toml
[analyzer]
auto_detect = true                                  # default: true
enabled     = ["cpp", "markdown", "python", "tera"] # instances to run

[analyzer.cpp]
include_paths = ["include", "src"]
```

Only analyzers listed under `[analyzer.X]` (or `enabled`) are instantiated — there is no global "all analyzers always run" mode.

### Auto-detection

An analyzer runs if:

1. It is declared (listed in `enabled` or configured via `[analyzer.X]`).
2. AND either `auto_detect = false`, OR the analyzer detects relevant files in the project.

This mirrors how processors work.

## Caching

Analyzer results are cached in the dependency cache (`.rsconstruct/deps.redb`). On subsequent builds:

- If neither a source file nor any of its cached dependencies has changed, the cached dependencies are used.
- If the source or any dependency has changed, or a dependency is gone, the source is re-scanned. Checking the dependencies is what catches a header that gains a new `#include`: the sources that include it are rescanned and pick up the new header.
- If a file appears where resolution previously found nothing — a `src/x.h` that now shadows the `include/x.h` an `#include "x.h"` used to resolve to, or a local `foo.py` for an `import foo` that used to be external — the source is re-scanned. Analyzers record the paths they probed and found missing.
- If the analyzer's configuration changes (`include_paths`, `load_paths`, pkg-config output, ...), every source it scanned is re-scanned: an entry is only trusted under the configuration it was scanned with.
- The `sass` and `tera` analyzers do not use the cached list at all: they rescan every source on every build.
- The cache is shared across all analyzers.

## Generated files

A reference to a file that another product generates is a dependency even
before the file exists: analyzers resolve references against the project's
files plus every declared output. The product that references it is ordered
after the product that generates it.

A source that is itself generated (or that includes a generated header) is
analyzed again during the build, right after the product generating it ran.
On a clean checkout that is the first time its content can be read; on an
incremental build it replaces the analysis of the stale copy that was on
disk when the graph was built. Either way the products reading it are
ordered, checksummed and cached against what they will actually read, and
the build after a clean build finds everything up to date.

The `cpp` analyzer asks the compiler (`-MM`), which only sees files on disk,
so it cannot resolve an include of a header that has not been generated
yet; use `icpp` for projects that generate headers. The `icpp`, `markdown`,
`sass` and `tera` analyzers all resolve generated files. The tera
functions that query a directory (`glob`, `grep_count`, `shell_output`'s
`depends_on`, `workflow_names`) see only the files on disk.

Use the `analyzer` command to manage analyzers and inspect the cache (full
reference in [Commands](commands.md#rsconstruct-analyzer)):

```bash
rsconstruct analyzer list                   # list available analyzers
rsconstruct analyzer defconfig cpp          # show default config for an analyzer
rsconstruct analyzer add cpp                # append [analyzer.cpp] to rsconstruct.toml with comments
rsconstruct analyzer add cpp --dry-run      # preview without writing
rsconstruct analyzer show all               # show all cached dependencies
rsconstruct analyzer show files src/main.c  # show dependencies for specific files
rsconstruct analyzer clean                  # clear the dependency cache
```

## Build phases

With `--phases`, you can see when analyzers run:

```bash
rsconstruct --phases build
```

Output:

```
Phase: Building dependency graph...
  Phase: discover
  Phase: add_dependencies    # Analyzers run here
  Phase: apply_tool_version_hashes
  Phase: resolve_dependencies
```

Use `--stop-after add-dependencies` to stop after dependency analysis:

```bash
rsconstruct build --stop-after add-dependencies
```

## Adding a custom analyzer

Analyzers implement the `DepAnalyzer` trait:

```rust
pub trait DepAnalyzer: Sync + Send {
    fn description(&self) -> &str;
    fn auto_detect(&self, file_index: &FileIndex) -> bool;
    fn match_product(&self, product: &Product) -> Option<PathBuf>;
    fn scan(&self, ctx: &BuildContext, source: &Path, file_index: &FileIndex)
        -> Result<ScanResult>;
    fn fingerprint_parts(&self, ctx: &BuildContext) -> Result<Vec<String>>;
    fn always_rescan(&self) -> bool { false }
}
```

An analyzer only answers two questions: which source a product's
dependencies come from (`match_product`), and what one source depends on
(`scan`). Everything else — grouping products by source, the dependency
cache, adding inputs to the graph, re-analysis of generated sources — is
done by the framework (`analyzers::Analysis`), the same way for every
analyzer.

`scan` returns a `ScanResult`:

- `deps` — the files the source depends on. Resolve against `file_index`
  as well as the disk: it lists files other products will generate.
- `absent` — every path resolution probed and found missing before its
  answer. If one of them appears later, the cached result is dropped.
- `hash_pieces` — non-file state to mix into the cache key (glob results,
  command text). Only allowed with `always_rescan`, since it is never cached.

`fingerprint_parts` returns everything besides file contents that decides
what `scan` resolves — normally the serialized config
(`analyzers::config_fingerprint`), plus any search paths resolved at run
time.
