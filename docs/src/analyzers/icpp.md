# icpp

Native (no-subprocess) C/C++ dependency analyzer. Scans `#include` directives by parsing source files directly in Rust, without invoking `gcc` or `pkg-config`.

**Native**: Yes.

**Auto-detects**: Projects with `.c`, `.cc`, `.cpp`, `.cxx`, `.h`, `.hh`, `.hpp`, or `.hxx` files.

## When to use

- You want faster analysis without the overhead of launching `gcc` per file.
- You don't need compiler-driven include path discovery.
- You're happy to enumerate include paths explicitly in `rsconstruct.toml`.

Prefer [cpp](cpp.md) if you need compiler-discovered system include paths or pkg-config integration.

## Configuration

```toml
[analyzer.icpp]
include_paths          = ["include", "src"]
src_exclude_dirs       = ["/kernel/", "/vendor/"]
follow_angle_brackets  = false
skip_not_found         = false
```

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
