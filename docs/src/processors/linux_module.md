# Linux Module Processor

## Purpose

Builds Linux kernel modules (`.ko` files) from source, driven by a
`linux-module.yaml` manifest. The processor generates a temporary `Kbuild`
file, invokes the kernel build system (`make -C <kdir> M=<src> modules`),
copies the resulting `.ko` to the output directory, and cleans up build
artifacts from the source tree.

## How It Works

The processor scans for `linux-module.yaml` files. Each manifest lists one
or more kernel modules to build. For each module the processor:

1. Generates a `Kbuild` file in the source directory (next to the yaml).
2. Runs `make -C <kdir> M=<absolute-source-dir> modules` to compile.
3. Copies the `.ko` file to `out/linux-module/<yaml-relative-dir>/`.
4. Runs `make ... clean` and removes the generated `Kbuild` so the source
   directory stays clean.

Because the kernel build system requires `M=` to point at an absolute path
containing the sources and `Kbuild`, the make command runs in the yaml
file's directory — not the project root.

The processor is a **generator**: it knows exactly which `.ko` files it
produces. Outputs are tracked in the build graph, cached in the object
store, and can be restored from cache after `rsconstruct clean` without
recompiling.

## linux-module.yaml Format

All source paths are relative to the yaml file's directory.

```yaml
# Global settings (all optional)
make: make                    # Make binary (default: "make")
kdir: /lib/modules/6.8.0-generic/build  # Kernel build dir (default: running kernel)
arch: x86_64                  # ARCH= value (optional, omitted if unset)
cross_compile: x86_64-linux-gnu-  # CROSS_COMPILE= value (optional)
v: 0                          # Verbosity V= (default: 0)
w: 1                          # Warning level W= (default: 1)

# Module definitions
modules:
  - name: hello               # Module name -> produces hello.ko
    sources: [main.c]         # Source files (relative to yaml dir)
    extra_cflags: [-DDEBUG]   # Extra CFLAGS (optional, becomes ccflags-y)

  - name: mydriver
    sources: [mydriver.c, utils.c]
```

### Minimal Example

A single module with one source file:

```yaml
modules:
  - name: hello
    sources: [main.c]
```

## Output Layout

Output is placed under `out/linux-module/<yaml-relative-dir>/`:

```
out/linux-module/<yaml-dir>/
  <module_name>.ko
```

For example, a manifest at `src/kernel/hello/linux-module.yaml` defining
module `hello` produces:

```
out/linux-module/src/kernel/hello/hello.ko
```

## KDIR Detection

If `kdir` is not set in the manifest, the processor runs `uname -r` to
detect the running kernel and uses `/lib/modules/<release>/build`. This
requires the `linux-headers-*` package to be installed (e.g.,
`linux-headers-generic` on Ubuntu).

## Generated Kbuild

The processor writes a `Kbuild` file with the standard kernel module
variables:

```makefile
obj-m := hello.o
hello-objs := main.o
ccflags-y := -DDEBUG       # only if extra_cflags is non-empty
```

This file is removed after building (whether the build succeeds or fails).

## Configuration

```toml
[processor.linux_module]
enabled = true           # Enable/disable (default: true)
extra_inputs = []        # Extra files that trigger rebuilds
```

### Configuration Reference

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `enabled` | bool | `true` | Enable/disable the processor |
| `extra_inputs` | string[] | `[]` | Extra files that trigger rebuilds when changed |
| `scan_dir` | string | `""` | Directory to scan for linux-module.yaml files |
| `extensions` | string[] | `["linux-module.yaml"]` | File patterns to scan for |
| `exclude_dirs` | string[] | common excludes | Directories to skip during scanning |

## Caching

The `.ko` outputs are cached in the rsconstruct object store. After `rsconstruct clean`,
a subsequent `rsconstruct build` restores `.ko` files from cache (via hardlink or
copy) without invoking the kernel build system. A rebuild is triggered when
any source file or the yaml manifest changes.

## Prerequisites

- `make` must be installed
- Kernel headers must be installed for the target kernel version
  (`apt install linux-headers-generic` on Ubuntu)
- For cross-compilation, the appropriate cross-compiler toolchain must be
  available and specified via `cross_compile` and `arch` in the manifest

## Example

Given this project layout:

```
myproject/
  rsconstruct.toml
  drivers/
    hello/
      linux-module.yaml
      main.c
```

With `drivers/hello/linux-module.yaml`:

```yaml
modules:
  - name: hello
    sources: [main.c]
```

And `drivers/hello/main.c`:

```c
#include <linux/module.h>
#include <linux/init.h>

MODULE_LICENSE("GPL");

static int __init hello_init(void) {
    pr_info("hello: loaded\n");
    return 0;
}

static void __exit hello_exit(void) {
    pr_info("hello: unloaded\n");
}

module_init(hello_init);
module_exit(hello_exit);
```

Running `rsconstruct build` produces:

```
out/linux-module/drivers/hello/hello.ko
```

The module can then be loaded with `sudo insmod out/linux-module/drivers/hello/hello.ko`.
