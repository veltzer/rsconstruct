# Getting Started

This guide walks through setting up an rsconstruct project for the two primary supported languages: Python and C++.

## Python

### Prerequisites

- rsconstruct installed ([Installation](installation.md))
- [ruff](https://docs.astral.sh/ruff/) on PATH

### Setup

Create a project directory and configuration:

```bash
mkdir myproject && cd myproject
```

```toml
# rsconstruct.toml
[processor.checker.ruff]
src_dirs = ["src"]
```

Every processor's `src_dirs` defaults to `[]` — it scans nothing until you say where its files live, so a stanza without `src_dirs` builds nothing.

Create a Python source file:

```bash
mkdir -p src
```

```python
# src/hello.py
def greet(name: str) -> str:
    return f"Hello, {name}!"

if __name__ == "__main__":
    print(greet("world"))
```

Run the build:

```bash
rsconstruct build
```

Expected output:

```text
[build] 1 products to check for updates
[build] 1 to build, 0 to restore (0 up-to-date)
[build] summary: 1 built, 0 restored, 0 failed (0 unchanged, 0 flaky)
```

Run again — nothing has changed, so rsconstruct skips the check:

```text
[build] 1 products to check for updates
[build] 0 to build, 0 to restore (1 up-to-date)
[build] summary: 0 built, 0 restored, 0 failed (1 unchanged, 0 flaky)
```

Add `-v` to see one line per product (`Processing: src/hello.py`, or the skip/restore reason).

### Adding pylint

Install [pylint](https://pylint.readthedocs.io/) and add a section for it:

```toml
# rsconstruct.toml
[processor.checker.ruff]
src_dirs = ["src"]

[processor.checker.pylint]
src_dirs = ["src"]
```

Pass extra arguments via processor config:

```toml
[processor.checker.pylint]
src_dirs = ["src"]
args = ["--disable=C0114,C0115,C0116"]
```

### Adding zspell for docs

If your project has markdown documentation, add a section for the zspell processor:

```toml
[processor.checker.ruff]
src_dirs = ["src"]

[processor.checker.pylint]
src_dirs = ["src"]

[processor.checker.zspell]
src_dirs = ["docs"]
```

Create a `.zspell-words` file in the project root with any custom words (one per line) that the zspeller should accept.

## C++

### Prerequisites

- rsconstruct installed ([Installation](installation.md))
- gcc/g++ on PATH

### Setup

Create a project directory and configuration:

```bash
mkdir myproject && cd myproject
```

```toml
# rsconstruct.toml
[processor.generator.cc_single_file]
src_dirs = ["src"]
```

Create a source file under `src/`:

```bash
mkdir -p src
```

```c
// src/hello.c
#include <stdio.h>

int main() {
    printf("Hello, world!\n");
    return 0;
}
```

Run the build:

```bash
rsconstruct build
```

Expected output:

```text
[build] 1 products to check for updates
[build] 1 to build, 0 to restore (0 up-to-date)
[build] summary: 1 built (1 files created), 0 restored, 0 failed (0 unchanged, 0 flaky)
```

The compiled executable is at `out/processor.generator.cc_single_file/src/hello.elf` — the output directory mirrors the source path.

Run again — the source hasn't changed, so the product is up to date:

```text
[build] 1 products to check for updates
[build] 0 to build, 0 to restore (1 up-to-date)
[build] summary: 0 built, 0 restored, 0 failed (1 unchanged, 0 flaky)
```

If the executable is deleted, the next build restores it from the cache instead of recompiling (`-v` shows `Restored from cache: ...`).

### Customizing compiler flags

Pass flags via processor config:

```toml
[processor.generator.cc_single_file]
src_dirs = ["src"]
cflags = ["-Wall", "-Wextra", "-O2"]
cxxflags = ["-Wall", "-Wextra", "-O2"]
include_paths = ["include"]
```

See the [CC Single File](processor/generator/cc_single_file.md) processor docs for the full configuration reference.

### Adding static analysis

Install [cppcheck](http://cppcheck.net/) and add a section for it:

```toml
[processor.generator.cc_single_file]
src_dirs = ["src"]

[processor.checker.cppcheck]
src_dirs = ["src"]
```

Both processors run on the same source files — rsconstruct handles them independently.

## Next Steps

- [Commands](commands.md) — full list of rsconstruct commands
- [Configuration](configuration.md) — all configuration options
- [Processors](processors.md) — detailed docs for each processor
