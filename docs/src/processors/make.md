# Make Processor

## Purpose

Runs `make` in directories containing Makefiles. Each Makefile produces a stub
file on success, allowing RSBuild to track incremental rebuilds.

## How It Works

Discovers files named `Makefile` in the project. For each Makefile found, the
processor runs `make` (or a configured alternative) in the Makefile's directory.
A stub file is created on success.

### Directory-Level Inputs

The make processor treats **all files in the Makefile's directory (and
subdirectories)** as inputs. This means that if any file alongside the
Makefile changes — source files, headers, scripts, included makefiles — rsbuild
will re-run make.

This is slightly conservative: a change to a file that the Makefile does not
actually depend on will trigger a rebuild. In practice this is the right
trade-off because Makefiles can depend on arbitrary files and there is no
reliable way to know which ones without running make itself.

## Source Files

- Input: `**/Makefile` plus all files in the Makefile's directory tree
- Output: `out/make/{relative_path}.done`

## Dependency Tracking Approaches

RSBuild uses the directory-scan approach described above. Here is why, and what
the alternatives are.

### 1. Directory scan (current)

Track every file under the Makefile's directory as an input. Any change
triggers a rebuild.

**Pros:** simple, correct, zero configuration.
**Cons:** over-conservative — a change to an unrelated file in the same
directory triggers a needless rebuild.

### 2. User-declared extra inputs

The user lists specific files or globs in `extra_inputs`. Only those files
(plus the Makefile itself) are tracked.

**Pros:** precise, no unnecessary rebuilds.
**Cons:** requires the user to manually maintain the list. Easy to forget a
file and get stale builds.

This is available today via the `extra_inputs` config key, but on its own
it would miss source files that the Makefile compiles.

### 3. Parse `make --dry-run --print-data-base`

Ask make to dump its dependency database and extract the real inputs.

**Pros:** exact dependency information, no over-building.
**Cons:** fragile — output format varies across make implementations
(GNU Make, BSD Make, nmake). Some Makefiles behave differently in dry-run
mode. Complex to implement and maintain.

### 4. Hash the directory tree

Instead of listing individual files, compute a single hash over every file
in the directory. Functionally equivalent to option 1 but with a different
internal representation.

**Pros:** compact cache key.
**Cons:** same over-conservatism as option 1, and no ability to report
which file changed.

## Configuration

```toml
[processor.make]
make = "make"        # Make binary to use
args = []            # Extra arguments passed to make
target = ""          # Make target (empty = default target)
scan_dir = ""        # Directory to scan ("" = project root)
extensions = ["Makefile"]
extra_inputs = []    # Additional files that trigger rebuilds when changed
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `make` | string | `"make"` | Path or name of the make binary |
| `args` | string[] | `[]` | Extra arguments passed to every make invocation |
| `target` | string | `""` | Make target to build (empty = default target) |
| `scan_dir` | string | `""` | Directory to scan for Makefiles |
| `extensions` | string[] | `["Makefile"]` | File names to match |
| `exclude_paths` | string[] | `[]` | Paths (relative to project root) to exclude |
| `extra_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds (in addition to directory contents) |
