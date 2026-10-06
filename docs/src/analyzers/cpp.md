# cpp

Scans C/C++ source files for `#include` directives and adds header file dependencies to the build graph.

**Native**: No (may invoke `gcc`, `pkg-config`).

**Auto-detects**: Projects with `.c`, `.cc`, `.cpp`, `.cxx`, `.h`, `.hh`, `.hpp`, or `.hxx` files.

## Features

- Recursive header scanning (follows includes in header files)
- Queries compiler for system include paths (only tracks project-local headers)
- Handles both `#include "file"` (relative to source) and `#include <file>` (searches include paths)
- Supports native regex scanning and compiler-based scanning (`gcc -MM`)
- Uses the dependency cache for incremental builds

## System header detection

The cpp analyzer queries the compiler for its include search paths using `gcc -E -Wp,-v -xc /dev/null`. This allows it to properly identify which headers are system headers vs project-local headers. Only headers within the project directory are tracked as dependencies.

## Configuration

```toml
[analyzer.cpp]
include_scanner       = "native"          # or "compiler" for gcc -MM
include_paths         = ["include", "src"]
pkg_config            = ["gtk+-3.0", "libcurl"]
include_path_commands = ["gcc -print-file-name=plugin"]
src_exclude_dirs      = ["/kernel/", "/vendor/"]
cc                    = "gcc"
cxx                   = "g++"
cflags                = ["-I/usr/local/include"]
cxxflags              = ["-std=c++17"]
```

### `include_path_commands`

Shell commands whose stdout (trimmed) is added to the include search paths. Useful for compiler-specific include directories:

```toml
[analyzer.cpp]
include_path_commands = [
    "gcc -print-file-name=plugin",  # GCC plugin development headers
    "llvm-config --includedir",     # LLVM headers
]
```

### `pkg_config` integration

Runs `pkg-config --cflags-only-I` for each package and adds the resulting include paths to the search path. Useful when your code includes headers from system libraries:

```toml
[analyzer.cpp]
pkg_config = ["gtk+-3.0", "glib-2.0"]
```

This automatically finds headers like `<gtk/gtk.h>` and `<glib.h>` without manually specifying their include paths.

### Failures are errors

Both are resolved once, when the analyzer is set up, before any file is
scanned. Any failure stops the build with an error naming the analyzer and
the package or command: `pkg-config` missing, an unknown package, a command
that exits non-zero, prints nothing, or prints something that is not a
directory. (They used to be printed and skipped. The analyzer then ran
without those include directories, headers found through them dropped out
of the dependency lists, and editing one rebuilt nothing — with the build
still green.) A package that resolves to no `-I` flags at all is not an
error: its headers live in a default directory.

## See also

- [icpp](icpp.md) — native (no-subprocess) C/C++ dependency analyzer
