# CC Project Processor

## Purpose

Builds full C/C++ projects with multiple targets (libraries and executables)
defined in a `cc.yaml` manifest file. Unlike the [CC Single File](../generator/cc_single_file.md)
processor which compiles each source file into a standalone executable, this
processor supports multi-file targets with dependency linking.

## How It Works

The processor scans for `cc.yaml` files. Each manifest defines libraries
and programs to build. All paths in the manifest (sources, include directories)
are relative to the `cc.yaml` file's location and are automatically resolved
to project-root-relative paths before compilation. All commands run from the
project root.

Every file written under `out/processor.mass_generator.cc/<path-to-cc.yaml-dir>/`
(`obj/<target>/*.o`, `lib/lib<name>.a`, `lib/lib<name>.so`, and `bin/<name>`)
is planned in advance as its own cached product, built into a staging directory,
and verified against the plan via `install_staged`.

Source files are compiled to object files, then linked into the final targets:

```text
src/exercises/foo/cc.yaml defines:
  library "mymath" (static) from math.c, utils.c
  program "main" from main.c, links mymath

Build produces:
  out/processor.mass_generator.cc/src/exercises/foo/obj/mymath/math.o
  out/processor.mass_generator.cc/src/exercises/foo/obj/mymath/utils.o
  out/processor.mass_generator.cc/src/exercises/foo/lib/libmymath.a
  out/processor.mass_generator.cc/src/exercises/foo/obj/main/main.o
  out/processor.mass_generator.cc/src/exercises/foo/bin/main
```

## cc.yaml Format

All paths in the manifest are relative to the `cc.yaml` file's location.

```yaml
# Global settings (all optional). A field left unset inherits its value
# from [processor.mass_generator.cc] in rsconstruct.toml; a field set explicitly — even
# to an empty list — overrides the config default for this manifest.
cc: gcc               # C compiler (unset: inherit config, default gcc)
cxx: g++              # C++ compiler (unset: inherit config, default g++)
cflags: [-Wall]       # Global C flags (unset: inherit config)
cxxflags: [-Wall]     # Global C++ flags (unset: inherit config)
ldflags: []           # Global linker flags (unset: inherit config)
include_dirs: [include]  # Global -I paths (relative to cc.yaml location; unset: inherit config)

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

## Compile Flags

Each source is compiled with the flags of its own language, at both
levels: a C file gets the global `cflags` then the target's `cflags`, a C++
file gets the global `cxxflags` then the target's `cxxflags`. A target's
`cflags` never reach its C++ objects and vice versa. After the language
flags come `-fPIC` (objects of a shared library only), then `-I` for the
target's `include_dirs` and after them the global `include_dirs`:

```text
<global cflags|cxxflags> <target cflags|cxxflags> [-fPIC] -I<target dirs> -I<global dirs>
```

## Header Dependencies

The manifest is the product's first input and the sources it lists follow
it. With the [cpp](../../analyzers/cpp.md) or
[icpp](../../analyzers/icpp.md) analyzer enabled, every listed source is
scanned for `#include`s and its headers become inputs of the objects,
libraries and programs built from it, so editing a header rebuilds what
includes it. The analyzer resolves includes with its own `include_paths`,
not the manifest's `include_dirs`; list the same directories there.

## Output Layout

Output is placed under `out/processor.mass_generator.cc/<cc.yaml-relative-dir>/`:

```text
out/processor.mass_generator.cc/<cc.yaml-dir>/
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

When `single_invocation = true` in `rsconstruct.toml`, programs are built by passing
all sources directly to the compiler in one command. Libraries still use
compile+link since `ar` requires object files.

One command has one language: when any source is C++ the C++ driver
compiles every file as C++ and the command carries the `cxxflags` (global,
then the program's); otherwise the `cflags`. Include directories and the
program's `ldflags` are passed as in compile+link mode.

## Configuration

```toml
[processor.mass_generator.cc]
enabled = true            # Enable/disable (default: true)
cc = "gcc"                # Default C compiler (default: "gcc")
cxx = "g++"               # Default C++ compiler (default: "g++")
cflags = []               # Additional global C flags
cxxflags = []             # Additional global C++ flags
ldflags = []              # Additional global linker flags
include_dirs = []         # Additional global -I paths
single_invocation = false # Use single-invocation mode (default: false)
dep_inputs = []           # Extra files that trigger rebuilds
```

The compiler and flag fields are project-wide **defaults** for every
`cc.yaml`: a manifest that leaves a field unset inherits the config value,
and a manifest that sets it explicitly (even to an empty list) overrides it
for that directory. Compilers overridden per manifest are picked up by tool
checking during discovery.

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
| `dep_inputs` | string[] | `[]` | Extra files that trigger rebuilds when changed |
| `src_dirs` | string[] | `[]` | Directories to scan for cc.yaml files (`[""]` for the whole project) |
| `src_extensions` | string[] | `["cc.yaml"]` | File patterns to scan for |

## Batch support

Runs once per `cc.yaml` manifest when any of its planned output files is dirty.

## Example

Given this project layout:

```text
myproject/
  rsconstruct.toml
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

Running `rsconstruct build` produces:

```text
out/processor.mass_generator.cc/exercises/math/obj/math/math.o
out/processor.mass_generator.cc/exercises/math/lib/libmath.a
out/processor.mass_generator.cc/exercises/math/obj/main/main.o
out/processor.mass_generator.cc/exercises/math/bin/main
```

## Clean behavior

This processor is a Mass Generator — `rsconstruct clean outputs` removes each planned output file (`product.outputs`) and then sweeps any parent directories that are now empty. Pass `--no-empty-dirs` to keep them. See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
