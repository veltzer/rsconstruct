# CC Project Processor

## Purpose

Builds full C/C++ projects with multiple targets (libraries and executables)
defined in a `cc.yaml` manifest file. Unlike the [CC Single File](cc_single_file.md)
processor which compiles each source file into a standalone executable, this
processor supports multi-file targets with dependency linking.

## How It Works

The processor scans for `cc.yaml` files. Each manifest defines libraries
and programs to build. All paths in the manifest (sources, include directories)
are relative to the `cc.yaml` file's location and are automatically resolved
to project-root-relative paths before compilation. All commands run from the
project root.

Output goes under `out/cc/<path-to-cc.yaml-dir>/`, so a manifest at
`src/exercises/foo/cc.yaml` produces output in `out/cc/src/exercises/foo/`.
A manifest at the project root produces output in `out/cc/`.

Source files are compiled to object files, then linked into the final targets:

```
src/exercises/foo/cc.yaml defines:
  library "mymath" (static) from math.c, utils.c
  program "main" from main.c, links mymath

Build produces:
  out/cc/src/exercises/foo/obj/mymath/math.o
  out/cc/src/exercises/foo/obj/mymath/utils.o
  out/cc/src/exercises/foo/lib/libmymath.a
  out/cc/src/exercises/foo/obj/main/main.o
  out/cc/src/exercises/foo/bin/main
```

## cc.yaml Format

All paths in the manifest are relative to the `cc.yaml` file's location.

```yaml
# Global settings (all optional)
cc: gcc               # C compiler (default: gcc)
cxx: g++              # C++ compiler (default: g++)
cflags: [-Wall]       # Global C flags
cxxflags: [-Wall]     # Global C++ flags
ldflags: []           # Global linker flags
include_dirs: [include]  # Global -I paths (relative to cc.yaml location)

# Library definitions
libraries:
  - name: mymath
    lib_type: shared   # shared (.so) | static (.a) | both
    sources: [src/math.c, src/utils.c]
    include_dirs: [include]  # Additional -I for this library
    cflags: []               # Additional C flags
    cxxflags: []             # Additional C++ flags
    ldflags: [-lm]           # Linker flags for shared lib

  - name: myhelper
    lib_type: static
    sources: [src/helper.c]

# Program definitions
programs:
  - name: main
    sources: [src/main.c]
    link: [mymath, myhelper]  # Libraries defined above to link against
    ldflags: [-lpthread]      # Additional linker flags

  - name: tool
    sources: [src/tool.cc]    # .cc -> uses C++ compiler
    link: [mymath]
```

## Library Types

| Type | Output | Description |
|------|--------|-------------|
| `shared` | `lib/lib<name>.so` | Shared library (default). Sources compiled with `-fPIC`. |
| `static` | `lib/lib<name>.a` | Static library via `ar rcs`. |
| `both` | Both `.so` and `.a` | Builds both shared and static variants. |

## Language Detection

The compiler is chosen per source file based on extension:

| Extensions | Compiler |
|-----------|----------|
| `.c` | C compiler (`cc` field) |
| `.cc`, `.cpp`, `.cxx`, `.C` | C++ compiler (`cxx` field) |

Global `cflags` are used for C files and `cxxflags` for C++ files.

## Output Layout

Output is placed under `out/cc/<cc.yaml-relative-dir>/`:

```
out/cc/<cc.yaml-dir>/
  obj/<target_name>/    # Object files per target
    file.o
  lib/                  # Libraries
    lib<name>.a
    lib<name>.so
  bin/                  # Executables
    <program_name>
```

## Build Modes

### Compile + Link (default)

Each source is compiled to a `.o` file, then targets are linked from objects.
This provides incremental rebuilds — only changed sources are recompiled.

### Single Invocation

When `single_invocation = true` in `rsbuild.toml`, programs are built by passing
all sources directly to the compiler in one command. Libraries still use
compile+link since `ar` requires object files.

## Configuration

```toml
[processor.cc]
enabled = true            # Enable/disable (default: true)
cc = "gcc"                # Default C compiler (default: "gcc")
cxx = "g++"               # Default C++ compiler (default: "g++")
cflags = []               # Additional global C flags
cxxflags = []             # Additional global C++ flags
ldflags = []              # Additional global linker flags
include_dirs = []         # Additional global -I paths
single_invocation = false # Use single-invocation mode (default: false)
extra_inputs = []         # Extra files that trigger rebuilds
cache_output_dir = true   # Cache entire output directory (default: true)
```

Note: The `cc.yaml` manifest settings override the `rsbuild.toml` defaults for
compiler and flags.

### Configuration Reference

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `enabled` | bool | `true` | Enable/disable the processor |
| `cc` | string | `"gcc"` | Default C compiler |
| `cxx` | string | `"g++"` | Default C++ compiler |
| `cflags` | string[] | `[]` | Global C compiler flags |
| `cxxflags` | string[] | `[]` | Global C++ compiler flags |
| `ldflags` | string[] | `[]` | Global linker flags |
| `include_dirs` | string[] | `[]` | Global include directories |
| `single_invocation` | bool | `false` | Build programs in single compiler invocation |
| `extra_inputs` | string[] | `[]` | Extra files that trigger rebuilds when changed |
| `cache_output_dir` | bool | `true` | Cache the entire output directory |
| `scan_dir` | string | `""` | Directory to scan for cc.yaml files |
| `extensions` | string[] | `["cc.yaml"]` | File patterns to scan for |

## Example

Given this project layout:

```
myproject/
  rsbuild.toml
  exercises/
    math/
      cc.yaml
      include/
        math.h
      math.c
      main.c
```

With `exercises/math/cc.yaml`:

```yaml
include_dirs: [include]

libraries:
  - name: math
    lib_type: static
    sources: [math.c]

programs:
  - name: main
    sources: [main.c]
    link: [math]
```

Running `rsbuild build` produces:

```
out/cc/exercises/math/obj/math/math.o
out/cc/exercises/math/lib/libmath.a
out/cc/exercises/math/obj/main/main.o
out/cc/exercises/math/bin/main
```
