# Glob-aware Tera dependencies

## The problem

Some Tera templates depend not on the contents of any one file, but on the
*set of files* that satisfy a pattern. The motivating example is from
[teaching-slides](https://github.com/veltzer/teaching-slides):

```jinja
- Currently there are {{ shell_output(command="find marp/courses -type f -name '*.md' -printf '%h\n' | sort -u | wc -l") }} courses in this repo.
- Currently there are {{ shell_output(command="find marp/lectures -type f -name '*.md' | wc -l") }} lectures in this repo.
- Currently there are {{ git_count_files(pattern="marp/**/*.md") }} marp files in this repo.
- Currently there are {{ shell_output(command="python3 scripts/count_slides.py") }} marp slides in this repo.
- Currently there are {{ git_count_files(pattern="svg/**/*.svg") }} SVG diagrams in this repo.
- Currently there are {{ shell_output(command="grep -r '^|.*---.*|' marp/ --include='*.md' | wc -l") }} tables in this repo.
```

The rendered output changes when files under `marp/` and `svg/` are
**added, removed, or renamed**, even though no specific file has been
edited and no path is listed as a dependency. rsconstruct today tracks
content-addressed dependencies: a product is rebuilt when the contents of
its declared inputs change. Add or remove a `.md` file under `marp/` and
the template product is happily served from cache, even though its
rendered output is now stale.

This is a fundamental "the build action depends on the directory listing,
not on a fixed set of files" problem. We need a way to express that.

## How other tools solve it

| Tool | Approach |
|---|---|
| **make** | Doesn't solve it. Globs in Makefiles are evaluated once, when the Makefile is parsed. Re-running `make` after adding a file picks up the new file because the Makefile is re-parsed. Make's dependency model = file content + mtime, never directory contents. |
| **ninja** | Same as make — relies on the build-graph generator (cmake, gn) to re-run and produce new `build.ninja`. |
| **bazel / buck** | Solve it cleanly by **forbidding implicit globs inside actions**. Globs are evaluated by the build tool itself at graph-construction time, producing a fixed list of files baked into the action. Add a file → BUILD-file evaluation produces a different action with a different cache key. The action itself never reads the directory. This works because Bazel **owns the `glob()` function** — it's not a side effect inside the user's tool. |
| **ninja + depfiles** | The tool emits a `.d` file at build time listing every input it actually read. Doesn't help here because pandoc/tera/sh don't know they're "reading the directory". |
| **tup** | Hooks the build with FUSE/ptrace and observes every `open()` and `readdir()` call. Captures directory listings as true dependencies. Heavyweight; cross-platform pain. |
| **directory mtime** | Cheap and approximate. A directory's mtime updates when entries are added or removed. Catches add/remove. Does not catch rename within a directory if the new name still hashes the same number of files; does not catch content-dependent queries (`grep -r`). |

## Chosen approach: Bazel-style, ported to Tera

Stop letting templates make uncontrolled directory queries. Promote
directory queries to first-class rsconstruct constructs that participate
in the dependency graph.

Three layers:

### 1. New Tera function: `glob(pattern=...)`

Templates that today say:

```jinja
{{ shell_output(command="find marp/courses -type f -name '*.md' | wc -l") }}
{{ git_count_files(pattern="marp/**/*.md") }}
```

are migrated to:

```jinja
{{ glob(pattern="marp/courses/**/*.md") | length }}
{{ glob(pattern="marp/**/*.md") | length }}
```

`glob()` is implemented inside rsconstruct's Tera engine. It does two things:

1. **Expands the glob** using rsconstruct's `FileIndex` — the same
   mechanism the rest of the build uses, so it respects `.gitignore` and
   `.rsconstructignore` and behaves consistently with `src_dirs`.
2. **Records the pattern as a graph-level dependency** on the calling
   product. Specifically:
   - Every matched file is added to `product.inputs` (its content checksum
     contributes to the product's input checksum).
   - The sorted list of resolved paths is hashed into a **glob-set
     fingerprint** which is mixed into the product's `config_hash`.

The fingerprint matters because rsconstruct's cache key is content-addressed
([Cache System](cache.md)) — renaming a file with identical content
otherwise wouldn't bust the cache. Mixing the sorted-path fingerprint into
the config hash makes add / remove / rename all flip the cache key
deterministically.

Returns a list of file paths as strings. Templates can use Tera's built-in
`length`, `sort`, iteration, etc. on the result.

### 2. Tightened `shell_output(command=..., depends_on=[...])`

For users who genuinely need to shell out for content-dependent results
(`grep -r '...' marp/`), require an explicit `depends_on` list of glob
patterns:

```jinja
{{ shell_output(
    command="grep -r '^|.*---.*|' marp/ --include='*.md' | wc -l",
    depends_on=["marp/**/*.md"]
) }}
```

`shell_output` then:

- Resolves every pattern in `depends_on`, adding matched files to
  `product.inputs`.
- Mixes the sorted union of resolved paths into the product's config hash.
- Mixes the literal command string into the product's config hash so
  command edits also rebuild.
- Runs the command and returns its trimmed stdout (current behavior).

A `shell_output` call **with no `depends_on`** is rejected at template-render
time:

> shell_output() requires a `depends_on=[...]` list of glob patterns.
> rsconstruct cannot otherwise tell when its output should be invalidated.
> If your command genuinely has no file dependencies, pass `depends_on=[]`
> explicitly to acknowledge that.

The empty-list escape hatch is for commands like `date` or `whoami` whose
output rsconstruct can never track — the user takes responsibility.

### 3. The Tera analyzer extends to scan for these calls

[`src/analyzers/tera.rs`](https://github.com/veltzer/rsconstruct/blob/master/src/analyzers/tera.rs)
already scans templates for `{% include %}`, `{% import %}`, `{% extends %}`,
and `load_lua/load_data/...` directives. Extend it to also recognize:

- `glob(pattern="...")` — captures `pattern`, expands to file list.
- `shell_output(..., depends_on=[...])` — captures the literal pattern list,
  expands each to file list. The command string itself is captured into
  `config_hash` (not `inputs`).

Captured patterns and their resolved file lists become per-product
dependency contributions, fed through the same machinery used today for
include/import dependencies. The analyzer cache (`.rsconstruct/deps.redb`)
gets one entry per template per pattern, so re-runs on unchanged templates
are O(1).

## Why this is the right shape

- **Composes with existing machinery.** No new caching layer; everything
  flows through `product.inputs`, `config_hash`, and the existing
  content-addressed cache key. The only new piece is the glob-set
  fingerprint, which is a one-liner `sha256(sorted_paths.join("\n"))`.
- **Catches all three failure modes.** Add → new path in glob set → new
  fingerprint. Remove → path missing → new fingerprint. Rename → both
  fingerprint and `inputs` change.
- **User opt-in is local.** The user adds a function call in one place;
  rsconstruct does the rest. Compare with `dep_inputs = [...]` in
  `rsconstruct.toml`, which scales poorly because every template that
  uses globbing would need a config update.
- **Failure of the old idiom is loud.** `shell_output` without `depends_on`
  becomes an error, not a silent stale build. Existing templates surface
  immediately when first re-rendered.
- **No filesystem instrumentation.** No FUSE, no ptrace, no eBPF. Stays
  cross-platform and inside the "simple, cross-platform" philosophy in
  [CLAUDE.md](https://github.com/veltzer/rsconstruct/blob/master/CLAUDE.md).

## Migration

The change is backwards-incompatible for `shell_output` calls without
`depends_on`. Two options:

1. **Hard cut-over.** Bump the processor version on `tera`. First build
   after upgrade fails fast on every legacy `shell_output` call with the
   error above. User migrates to `glob(...)` (preferred) or
   `shell_output(..., depends_on=[...])`.
2. **Soft transition.** Add a new function `tracked_shell_output(...,
   depends_on=...)` and leave `shell_output` working but emit a deprecation
   warning at render time. Remove `shell_output` after one release cycle.

We pick option 1. The library is pre-1.0 and the existing `shell_output`
is the source of incremental-build bugs we're fixing — making it loud is
the point. Per
[`feedback_no_backcompat`](../../../.claude/projects/-home-mark-git-veltzer-rsconstruct/memory/feedback_no_backcompat.md)
in our memory, on-disk and config back-compat aren't a concern at this
stage.

## Lower-effort fallback (not chosen)

Track the **mtime of each `src_dir`** as an automatic input for every
product whose processor scans that directory. Catches add/remove and most
rename cases, zero user-facing API change. Doesn't catch content-dependent
queries like `grep -r '...'`. Roughly 30 lines of code, no template-engine
changes. Could ship as a stop-gap if the full design proves too disruptive.

We chose against it because:

- mtime is unreliable on some filesystems and tar/git restores set it
  arbitrarily.
- It would silently miss the `grep -r` case from the motivating example.
- It papers over the real bug (`shell_output` is unsafe) without forcing
  users to think about what their templates actually depend on.

## Implementation

Status: **shipped**. Implemented as two pieces:

**Static analysis (the cache-key path)** — `src/analyzers/tera.rs` reads each
template file as text and regex-matches:

- `{% include "..." %}`, `{% import "..." %}`, `{% extends "..." %}` (existing)
- `load_lua/load_data/...(path="...")` (existing)
- `glob(pattern="...")` (new)
- `shell_output(command="...", depends_on=[...])` (new)

For each match, the analyzer:

1. Resolves any glob patterns via the `glob` crate.
2. Adds matched files to the product's `inputs` (so file-content changes
   are caught by the existing input-checksum mechanism).
3. Mixes a deterministic string into the product's `config_hash` via
   `Product::extend_config_hash()`. The string is built from the literal
   pattern plus the sorted list of resolved paths plus, for `shell_output`,
   the literal command text. Adding/removing/renaming a matching file flips
   the path list; editing the command text flips the command piece.

The analyzer rejects `shell_output(...)` without `depends_on` at
graph-construction time. The error message includes the source file path,
the offending command, and migration advice.

A new helper `analyzers::analyze_with_full_scanner` was added to support
analyzers whose scanner returns a [`ScanResult`] (deps + config-hash piece)
rather than just deps. The existing C/C++/Python/markdown analyzers
continue to use the simpler `analyze_with_scanner`.

**Runtime evaluation (the rendered-output path)** — `src/processors/generators/tera.rs`
registers two functions on the Tera engine:

- `glob(pattern="...")` — runs the same expansion the analyzer did and
  returns the sorted list as a Tera string array. Templates can use
  `length`, iterate, or `join` it.
- `shell_output(command="...", depends_on=[...])` — runs the command and
  returns its trimmed stdout. Validates that `depends_on` is present and is
  an array. Empty list is permitted (explicit user opt-out of file-level
  dependency tracking).

The runtime validation in `shell_output` is a defense-in-depth check: the
analyzer enforces `depends_on` at graph-construction time, but if for any
reason a template were rendered without going through the analyzer (e.g.,
during early prototyping or a misconfigured project) the render itself
would still surface the missing argument with a clear error.

**Caching note.** The path list returned by `scan_template` is *not* cached
in `.rsconstruct/deps.redb`. Tera analysis is cheap (regex + glob + path
hashing) and the config-hash piece needs to be recomputed on every build
anyway — its value depends on filesystem state, not on the template file's
content. A richer cache could be added later if profiling demands it.

### Files changed

- `src/graph.rs` — added `Product::extend_config_hash()`.
- `src/analyzers/mod.rs` — added `ScanResult` and `analyze_with_full_scanner`.
- `src/analyzers/tera.rs` — replaced `scan_includes` with `scan_template`,
  which handles all five constructs; added `expand_glob` helper.
- `src/processors/generators/tera.rs` — added `GlobFunction`; tightened
  `ShellOutputFunction` to require `depends_on`.
- `tests/processors/tera.rs` — 8 new tests covering count, add/remove/rename
  invalidation, missing `depends_on` rejection, content-change invalidation,
  command-edit invalidation, and empty-glob handling.

### Known limitations

- The `shell_output(...)` body regex stops at the first `)`. Commands or
  argument values that contain a literal `)` could confuse the analyzer.
  None of the existing teaching-slides commands hit this, but a
  quote-and-paren-aware parser would be a small follow-up if the limitation
  becomes painful.
- Globs are evaluated with the `glob` crate's syntax (POSIX-ish with `**`
  for recursive) and do not currently honor `.gitignore` /
  `.rsconstructignore`. For most users this is fine because the patterns
  are scoped to a known directory. If unwanted matches appear, exclude
  them with a more specific pattern.

## Open questions

- **What about `git_count_files`?** The existing function uses
  `git ls-files`, which is essentially a glob restricted to git-tracked
  files. We can keep it for backwards compatibility and have it record the
  same kind of dependency under the hood (the resolved file list from
  `git ls-files`), or we can just deprecate it in favor of `glob()`. The
  former is friendlier; do the former.
- **Caching the glob expansion itself.** `FileIndex` is already cached
  per-build. For repeated `glob()` calls inside one render the resolution
  is cheap. Across builds, the analyzer cache keys deps by source file, so
  if the `.tera` file hasn't changed and the source tree hasn't changed,
  the cached dep list is reused. No extra caching needed.
- **What if `glob()` matches zero files?** Return an empty list. Don't
  error — empty results are a legitimate state, and an error would force
  awkward `if-glob-empty-or` template logic.
