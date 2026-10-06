# Dependency Caching

RSConstruct includes a dependency cache that stores source file dependencies (e.g., C/C++ header files) to avoid re-scanning files that haven't changed. This significantly speeds up the graph-building phase for projects with many source files.

## Overview

When processors like `cc_single_file` discover products, they need to scan source files to find dependencies (header files). This scanning can be slow for large projects. The dependency cache stores the results so subsequent builds can skip the scanning step.

The cache is stored in `.rsconstruct/deps.redb` using [redb](https://github.com/cberner/redb), an embedded key-value database.

## Cache Structure

Each cache entry consists of:

- **Key**: Analyzer instance name and source file path (e.g., `icpp` + `src/main.c`)
- **Value**:
  - `source_checksum` — SHA-256 hash of the source file content
  - `dependencies` — list of dependency paths (header files)
  - `dependency_checksums` — SHA-256 hash of each dependency when it was scanned
  - `absent` — paths the scan probed and found missing on the way to its answer
  - `config_fingerprint` — hash of the analyzer's configuration (and run-time
    resolved search paths) the scan ran under

## Cache Lookup Algorithm

When looking up dependencies for a source file:

1. Look up the entry by source file path
2. If not found → cache miss, scan the file
3. If its `config_fingerprint` differs from the analyzer's current one →
   cache miss, re-scan
4. If found, compute the current SHA-256 checksum of the source file
5. Compare with the stored checksum:
   - If different → cache miss (file changed), re-scan
   - If same → check every cached dependency against its stored checksum
6. If any dependency is missing or its content changed → cache miss, re-scan
7. If any `absent` path now exists → cache miss, re-scan
8. Otherwise → cache hit, return cached dependencies

Step 5 is what keeps transitive lists correct. When `a.h` gains
`#include "b.h"`, `main.c` (which includes `a.h`) has not changed, but its
cached list `[a.h]` is now incomplete. `a.h`'s checksum no longer matches,
so `main.c` is rescanned and `b.h` joins its dependencies. Checking only
the source's own checksum, as the cache once did, left `b.h` out for good:
later edits to it never rebuilt `main.c`.

Checksums go through the mtime cache, so an unchanged dependency costs a
`stat`, not a read.

Steps 3 and 7 cover what decides a dependency list without being file
content. A resolution depends on which files are *missing*:
`#include "x.h"` resolved to `include/x.h` because `src/x.h` did not
exist, and once `src/x.h` appears it is the one the compiler finds. The
analyzer reports those probes (`ScanResult::absent`), and an entry is only
valid while all of them are still missing. It also depends on the
analyzer's configuration: adding an include path can change what every
`#include` resolves to, without any file changing. The `cpp` analyzer only
sees the compiler's answer, not its probes, so it reconstructs them: for
each header and each search directory it lies under, the same relative
spelling under every earlier directory (a superset of the real probes).

An entry is not stored when a dependency does not exist yet (a file
another product will generate): it has no checksum to validate against.
The source is scanned again once the file is generated.

## Why Path as Key (Not Checksum)?

An alternative design would use the source file's checksum as the cache key instead of its path. This seems appealing because you could look up dependencies directly by content hash. However, this approach has significant drawbacks:

### Problems with Checksum as Key

1. **Mandatory upfront computation**: With checksum as key, you must compute the SHA-256 hash of every source file before you can even check the cache. This means reading every file on every build, even when nothing has changed.

   With path as key, you do a fast O(1) lookup first. Only if there's a cache hit do you compute the checksum to validate freshness.

2. **Orphaned entries accumulate**: When a file changes, its old checksum entry becomes orphaned garbage. You'd need periodic garbage collection to clean up stale entries.

   With path as key, the entry is naturally updated in place when the file changes.

3. **No actual benefit**: The checksum is still needed for validation regardless of the key choice. Using it as the key just moves when you compute it, without reducing total work.

### Current Design

The current design is optimal:

```
Path (key) → O(1) lookup → Checksum validation (only on hit)
```

This minimizes work in the common case where files haven't changed.

## Cache Statistics

During graph construction, RSConstruct displays cache statistics:

```
[cc_single_file] Dependency cache: 42 hits, 3 recalculated
```

This shows how many source files had their dependencies retrieved from cache (hits) versus re-scanned (recalculated).

## Viewing Dependencies

Use the `rsconstruct analyzer` command to view the dependencies stored in the cache:

```bash
rsconstruct analyzer show all                    # Show all cached dependencies
rsconstruct analyzer show files src/main.c       # Show dependencies for a specific file
rsconstruct analyzer show files src/a.c src/b.c  # Show dependencies for multiple files
rsconstruct analyzer clean                       # Clear the dependency cache
```

Example output:

```
src/main.c: [icpp] (no dependencies)
src/test.c: [icpp]
  src/utils.h
  src/config.h
```

`analyzer show` reads directly from the dependency cache without building the graph. If the cache is empty (e.g., after `rsconstruct analyzer clean` or on a fresh checkout), run `rsconstruct analyzer build` or a build first to populate it.

This is useful for debugging rebuild behavior or understanding the include structure of your project.

## Cache Invalidation

The cache automatically invalidates entries when:

- The source file content changes (checksum mismatch)
- Any cached dependency file no longer exists, or its content changes
- A path the scan found missing now exists
- The analyzer's configuration changes

You can manually clear the entire dependency cache by removing the `.rsconstruct/deps.redb` file, or by running `rsconstruct clean all` which removes the entire `.rsconstruct/` directory.

## Processors Using Dependency Caching

Currently, the following processors use the dependency cache:

- **cc_single_file** — caches C/C++ header dependencies discovered by the include scanner

## Implementation

The dependency cache is implemented in `src/deps_cache.rs`:

```rust
pub struct DepsCache {
    db: redb::Database,
    stats: DepsCacheStats,
    pending: HashMap<String, DepsEntry>,
}

impl DepsCache {
    pub fn open() -> Result<Self>;
    pub fn get(&mut self, ctx, analyzer: AnalyzerId, source: &Path) -> Option<Vec<PathBuf>>;
    pub fn set(&mut self, ctx, analyzer: AnalyzerId, source: &Path, source_checksum: String,
               dependencies: &[PathBuf], absent: &[PathBuf]) -> Result<()>;
    pub fn flush(&mut self) -> Result<()>;
    pub fn stats(&self) -> &DepsCacheStats;
}
```

The cache is opened once per graph build and driven by `analyzers::Analysis`.
`set` only stages an entry (lookups see it at once); `flush` writes every
staged entry in one transaction, at the end of each analysis pass — the
graph-time pass and each mid-build re-analysis. A redb commit is durable, so
the old one-commit-per-`set` cost one fsync per scanned file: a cold scan of
2,000 sources went from 2,031 fsyncs to 32.
