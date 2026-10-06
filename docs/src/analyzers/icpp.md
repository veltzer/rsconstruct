# icpp

Native C/C++ dependency analyzer. Scans `#include` directives by parsing source files directly in Rust, without invoking `gcc`. It runs no subprocess unless `pkg_config` or `include_path_commands` is set; those run once, at setup, to find extra include directories.

**Native**: Yes.

**Auto-detects**: Projects with `.c`, `.cc`, `.cpp`, `.cxx`, `.h`, `.hh`, `.hpp`, or `.hxx` files.

## When to use

- You want faster analysis without the overhead of launching `gcc` per file.
- You don't need compiler-driven include path discovery.
- You're happy to enumerate include paths explicitly in `rsconstruct.toml`.

Prefer [cpp](cpp.md) if you need the compiler's own include resolution (macros, conditional includes, built-in system paths).

## Configuration

```toml
[analyzer.icpp]
include_paths          = ["include", "src"]
pkg_config             = ["gtk+-3.0"]
include_path_commands  = ["gcc -print-file-name=plugin"]
src_exclude_dirs       = ["/kernel/", "/vendor/"]
follow_angle_brackets  = false
skip_not_found         = false
```

### `pkg_config` and `include_path_commands`

The same as for [cpp](cpp.md#include_path_commands): directories from
`pkg-config --cflags-only-I` and from shell commands that each print one
directory are searched after `include_paths`. Any failure — `pkg-config`
missing, an unknown package, a command that fails, prints nothing, or
prints something that is not a directory — stops the build with an error
naming the package or command (see [cpp](cpp.md#failures-are-errors)).

### `follow_angle_brackets` (default: `false`)

Controls whether `#include <foo.h>` directives are followed.

- `false` (default) — angle-bracket includes are skipped entirely. System headers never enter the dependency graph, even when they resolve through configured include paths.
- `true` — angle-bracket includes are resolved and followed the same way as quoted includes. Unresolved angles are still tolerated (not an error), so missing system headers don't break analysis.

Quoted includes (`#include "foo.h"`) always resolve and must be found — this setting does not affect them (see `skip_not_found` below).

### `skip_not_found` (default: `false`)

Controls what happens when an include cannot be resolved.

- `false` (default) — a quoted include (`#include "foo.h"`) that cannot be resolved is a hard error. Unresolved angle-bracket includes are silently ignored (when `follow_angle_brackets = true`).
- `true` — unresolved includes of any kind are silently skipped.

Use `true` for partial / work-in-progress codebases where some headers aren't generated yet.

## See also

- [cpp](cpp.md) — compiler-aware (external) C/C++ dependency analyzer
