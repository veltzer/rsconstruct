# Dependency Caching

RSConstruct includes a dependency cache that stores source file dependencies (e.g., C/C++ header files) to avoid re-scanning files that haven't changed. This significantly speeds up the graph-building phase for projects with many source files.

## Overview

When processors like `cc_single_file` discover products, they need to scan source files to find dependencies (header files). This scanning can be slow for large projects. The dependency cache stores the results so subsequent builds can skip the scanning step.

The cache is stored in `.rsconstruct/deps.redb` using [redb](https://github.com/cberner/redb), an embedded key-value database.

## Cache Structure

Each cache entry consists of:

- **Key**: Source file path (e.g., `src/main.c`)
- **Value**:
  - `source_checksum` — SHA-256 hash of the source file content
  - `dependencies` — list of dependency paths (header files)

## Cache Lookup Algorithm

When looking up dependencies for a source file:

1. Look up the entry by source file path
2. If not found → cache miss, scan the file
3. If found, compute the current SHA-256 checksum of the source file
4. Compare with the stored checksum:
   - If different → cache miss (file changed), re-scan
   - If same → verify all cached dependencies still exist
5. If any dependency file is missing → cache miss, re-scan
6. Otherwise → cache hit, return cached dependencies

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

Use the `rsconstruct deps` command to view the dependencies stored in the cache:

```bash
rsconstruct deps all                    # Show all cached dependencies
rsconstruct deps for src/main.c         # Show dependencies for a specific file
rsconstruct deps for src/a.c src/b.c    # Show dependencies for multiple files
rsconstruct deps clean                  # Clear the dependency cache
```

Example output:

```
src/main.c: (no dependencies)
src/test.c:
  src/utils.h
  src/config.h
```

The `rsconstruct deps` command reads directly from the dependency cache without building the graph. If the cache is empty (e.g., after `rsconstruct deps clean` or on a fresh checkout), run a build first to populate it.

This is useful for debugging rebuild behavior or understanding the include structure of your project.

## Cache Invalidation

The cache automatically invalidates entries when:

- The source file content changes (checksum mismatch)
- Any cached dependency file no longer exists

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
}

impl DepsCache {
    pub fn open() -> Result<Self>;
    pub fn get(&mut self, source: &Path) -> Option<Vec<PathBuf>>;
    pub fn set(&self, source: &Path, dependencies: &[PathBuf]) -> Result<()>;
    pub fn flush(&self) -> Result<()>;
    pub fn stats(&self) -> &DepsCacheStats;
}
```

The cache is opened once per processor discovery phase, queried for each source file, and flushed to disk at the end.
