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

- If a source file hasn't changed, its cached dependencies are used.
- If a source file has changed, dependencies are re-scanned.
- The cache is shared across all analyzers.

Use the `analyzers` and `deps` commands to inspect the cache:

```bash
rsconstruct analyzers list            # list available analyzers
rsconstruct analyzers defconfig cpp   # show default config for an analyzer
rsconstruct analyzers add cpp         # append [analyzer.cpp] to rsconstruct.toml with comments
rsconstruct analyzers add cpp --dry-run  # preview without writing
rsconstruct deps all                  # show all cached dependencies
rsconstruct deps for src/main.c       # show dependencies for specific files
rsconstruct deps clean                # clear the dependency cache
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
    fn analyze(
        &self,
        graph: &mut BuildGraph,
        deps_cache: &mut DepsCache,
        file_index: &FileIndex,
        verbose: bool,
    ) -> Result<()>;
}
```

The `analyze` method should:

1. Find products with relevant source files.
2. Scan each source file for dependencies (using the cache when available).
3. Add discovered dependencies to the product's inputs.
