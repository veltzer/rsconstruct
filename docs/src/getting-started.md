# Getting Started

This guide walks through setting up an rsbuild project for the two primary supported languages: Python and C++.

## Python

### Prerequisites

- rsbuild installed ([Installation](installation.md))
- [ruff](https://docs.astral.sh/ruff/) on PATH

### Setup

Create a project directory and configuration:

```bash
mkdir myproject && cd myproject
```

```toml
# rsbuild.toml
[processor]
enabled = ["ruff"]
```

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
rsbuild build
```

Expected output:

```
Processing ruff (1 product)
  hello.py
```

Run again — nothing has changed, so rsbuild skips the check:

```
Processing ruff (1 product)
  Up to date
```

### Adding pylint

Install [pylint](https://pylint.readthedocs.io/) and add it to the enabled list:

```toml
# rsbuild.toml
[processor]
enabled = ["ruff", "pylint"]
```

Pass extra arguments via processor config:

```toml
[processor.pylint]
args = ["--disable=C0114,C0115,C0116"]
```

### Adding spellcheck for docs

If your project has markdown documentation, add the spellcheck processor:

```toml
[processor]
enabled = ["ruff", "pylint", "spellcheck"]
```

Create a `.spellcheck-words` file in the project root with any custom words (one per line) that the spellchecker should accept.

## C++

### Prerequisites

- rsbuild installed ([Installation](installation.md))
- gcc/g++ on PATH

### Setup

Create a project directory and configuration:

```bash
mkdir myproject && cd myproject
```

```toml
# rsbuild.toml
[processor]
enabled = ["cc_single_file"]
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
rsbuild build
```

Expected output:

```
Processing cc_single_file (1 product)
  hello.elf
```

The compiled executable is at `out/cc_single_file/hello.elf`.

Run again — the source hasn't changed, so rsbuild restores from cache:

```
Processing cc_single_file (1 product)
  Up to date
```

### Customizing compiler flags

Pass flags via processor config:

```toml
[processor.cc_single_file]
cflags = ["-Wall", "-Wextra", "-O2"]
cxxflags = ["-Wall", "-Wextra", "-O2"]
include_paths = ["include"]
```

See the [CC Single File](processors/cc_single_file.md) processor docs for the full configuration reference.

### Adding static analysis

Install [cppcheck](http://cppcheck.net/) and add it to the enabled list:

```toml
[processor]
enabled = ["cc_single_file", "cppcheck"]
```

Both processors run on the same source files — rsbuild handles them independently.

## Next Steps

- [Commands](commands.md) — full list of rsbuild commands
- [Configuration](configuration.md) — all configuration options
- [Processors](processors.md) — detailed docs for each processor
