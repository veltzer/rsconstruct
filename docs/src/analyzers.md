# Dependency Analyzers

rsb uses **dependency analyzers** to scan source files and discover dependencies between files. Analyzers run after processors discover products and add dependency information to the build graph.

## How Analyzers Work

1. **Product Discovery**: Processors discover products (source → output mappings)
2. **Dependency Analysis**: Analyzers scan source files to find dependencies
3. **Graph Resolution**: Dependencies are added to products for correct build ordering

Analyzers are decoupled from processors — they operate on any product with matching source files, regardless of which processor created it.

## Built-in Analyzers

### cpp

Scans C/C++ source files for `#include` directives and adds header file dependencies.

**Auto-detects**: Projects with `.c`, `.cc`, `.cpp`, `.cxx`, `.h`, `.hh`, `.hpp`, or `.hxx` files.

**Features**:
- Recursive header scanning (follows includes in header files)
- Filters out system headers (`/usr/`, `/lib/`)
- Supports both native regex scanning and compiler-based scanning (`gcc -MM`)
- Uses dependency cache for incremental builds

**Configuration** (`rsb.toml`):

```toml
[analyzer.cpp]
include_scanner = "native"  # or "compiler" for gcc -MM
include_paths = ["include", "src"]
cc = "gcc"
cxx = "g++"
cflags = ["-I/usr/local/include"]
cxxflags = ["-std=c++17"]
```

### python

Scans Python source files for `import` and `from ... import` statements and adds dependencies on local Python modules.

**Auto-detects**: Projects with `.py` files.

**Features**:
- Resolves imports to local files (ignores stdlib/external packages)
- Supports both `import foo` and `from foo import bar` syntax
- Searches relative to source file and project root

## Configuration

Analyzers can be configured in `rsb.toml`:

```toml
[analyzer]
auto_detect = true  # auto-detect which analyzers to run (default: true)
enabled = ["cpp", "python"]  # list of enabled analyzers
```

### Auto-detection

By default, analyzers use auto-detection to determine if they're relevant for the project. An analyzer runs if:
1. It's in the `enabled` list
2. AND either `auto_detect = false`, OR the analyzer detects relevant files

This is similar to how processors work.

## Caching

Analyzer results are cached in the dependency cache (`.rsb/deps/`). On subsequent builds:
- If a source file hasn't changed, its cached dependencies are used
- If a source file has changed, dependencies are re-scanned
- The cache is shared across all analyzers

Use `rsb deps` commands to inspect the cache:

```bash
rsb deps all                # Show all cached dependencies
rsb deps for src/main.c     # Show dependencies for specific files
rsb deps clean              # Clear the dependency cache
```

## Build Phases

With `--phases` flag, you can see when analyzers run:

```bash
rsb --phases build
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
rsb build --stop-after add-dependencies
```

## Adding Custom Analyzers

Analyzers implement the `DepAnalyzer` trait:

```rust
pub trait DepAnalyzer: Sync + Send {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn auto_detect(&self, file_index: &FileIndex) -> bool;
    fn analyze(
        &self,
        graph: &mut BuildGraph,
        deps_cache: &mut DepsCache,
        file_index: &FileIndex,
    ) -> Result<()>;
}
```

The `analyze` method should:
1. Find products with relevant source files
2. Scan each source file for dependencies (using cache when available)
3. Add discovered dependencies to the product's inputs
