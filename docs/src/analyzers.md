# Dependency Analyzers

rsconstruct uses **dependency analyzers** to scan source files and discover dependencies between files. Analyzers run after processors discover products and add dependency information to the build graph.

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
- Queries compiler for system include paths (only tracks project-local headers)
- Handles both `#include "file"` (relative to source) and `#include <file>` (searches include paths)
- Supports native regex scanning and compiler-based scanning (`gcc -MM`)
- Uses dependency cache for incremental builds

**System Header Detection**:

The cpp analyzer queries the compiler for its include search paths using `gcc -E -Wp,-v -xc /dev/null`. This allows it to properly identify which headers are system headers vs project-local headers. Only headers within the project directory are tracked as dependencies.

**Configuration** (`rsconstruct.toml`):

```toml
[analyzer.cpp]
include_scanner = "native"  # or "compiler" for gcc -MM
include_paths = ["include", "src"]
pkg_config = ["gtk+-3.0", "libcurl"]  # Query pkg-config for include paths
include_path_commands = ["echo $(gcc -print-file-name=plugin)/include"]  # Run commands to get include paths
exclude_dirs = ["/kernel/", "/vendor/"]  # Skip analyzing files in these directories
cc = "gcc"
cxx = "g++"
cflags = ["-I/usr/local/include"]
cxxflags = ["-std=c++17"]
```

**include_path_commands**:

The `include_path_commands` option allows you to specify shell commands that output include paths. Each command is executed and its stdout (trimmed) is added to the include search paths. This is useful for compiler-specific include directories:

```toml
[analyzer.cpp]
include_path_commands = [
    "gcc -print-file-name=plugin",  # GCC plugin development headers
    "llvm-config --includedir",     # LLVM headers
]
```

**pkg-config Integration**:

The `pkg_config` option allows you to specify pkg-config packages. The analyzer will run `pkg-config --cflags-only-I` to get the include paths for these packages and add them to the header search path. This is useful when your code includes headers from system libraries:

```toml
[analyzer.cpp]
pkg_config = ["gtk+-3.0", "glib-2.0"]
```

This will automatically find headers like `<gtk/gtk.h>` and `<glib.h>` without needing to manually specify their include paths.

### python

Scans Python source files for `import` and `from ... import` statements and adds dependencies on local Python modules.

**Auto-detects**: Projects with `.py` files.

**Features**:
- Resolves imports to local files (ignores stdlib/external packages)
- Supports both `import foo` and `from foo import bar` syntax
- Searches relative to source file and project root

## Configuration

Analyzers can be configured in `rsconstruct.toml`:

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

Analyzer results are cached in the dependency cache (`.rsconstruct/deps.redb`). On subsequent builds:
- If a source file hasn't changed, its cached dependencies are used
- If a source file has changed, dependencies are re-scanned
- The cache is shared across all analyzers

Use `rsconstruct deps` commands to inspect the cache:

```bash
rsconstruct deps all                # Show all cached dependencies
rsconstruct deps for src/main.c     # Show dependencies for specific files
rsconstruct deps clean              # Clear the dependency cache
```

## Build Phases

With `--phases` flag, you can see when analyzers run:

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

## Adding Custom Analyzers

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
    ) -> Result<()>;
}
```

The `analyze` method should:
1. Find products with relevant source files
2. Scan each source file for dependencies (using cache when available)
3. Add discovered dependencies to the product's inputs
