# Per-Processor Statistics

rsconstruct shows several "per-processor" or "per-analyzer" statistics tables
(`cache stats`, `analyzers stats`, `graph stats`, `build` summaries). These all
look similar on the surface, but the **data source differs**, and that changes
what we can cheaply show.

This document explains:

1. The three data sources that feed per-X statistics.
2. The per-processor grouping problem in `cache stats`.
3. Options for fixing it, with tradeoffs.
4. Secondary cleanup — graph-level helpers.

## The three data sources

| Question | Lives where | Cost of grouping by X |
|---|---|---|
| "How many products does pylint have in this build config?" | graph (in-memory) | free |
| "How many products were built / skipped / restored this run?" | executor stats (in-memory) | free |
| "How many files did each analyzer find?" | `.rsconstruct/deps.redb` (on disk, keyed by analyzer) | fast — single DB scan, key is already the analyzer name |
| "How big is my on-disk cache, per processor?" | `.rsconstruct/cache/descriptors/` (on disk) | **see below — this is the problem** |

### Graph (in-memory, rebuilt each run)

Every `Product` carries its `processor: String` field. Grouping is a simple
iteration over `Vec<Product>`, constructing a `HashMap<String, T>` on the spot.
Every caller that wants per-processor stats does this inline — see
`builder/graph.rs:111`, `builder/build.rs:323,436,467`,
`executor/execution.rs:180,479,524,540`.

### Analyzer dependency cache (`deps.redb`)

The redb schema stores each entry keyed by (source path → dependencies) and
tagged with the analyzer that produced it. `DepsCache::stats_by_analyzer()`
scans the DB once and returns `HashMap<analyzer, (file_count, dep_count)>`.
Grouping is effectively free because the analyzer name is a first-class field.

### Object-store descriptors (`.rsconstruct/cache/descriptors/`)

Each descriptor file is a small JSON blob describing one cached product — its
outputs, their checksums, etc. The **filename is a hash** of the product's
cache key; the file's location tells us nothing about which processor created
it.

Today's code in `object_store/management.rs:169`:

```rust
pub fn stats_by_processor(&self) -> BTreeMap<String, ProcessorCacheStats> {
    // walk every file in descriptors_dir
    //   read the file
    //   parse the JSON
    //   ...
    //   "We can't extract processor name from a hashed descriptor key.
    //    Use 'all' as a single bucket for now."
    let processor = "all".to_string();
}
```

Two things are wrong with this:

1. **It's a white lie.** The function is named `stats_by_processor`, but it
   returns a single `"all"` bucket. There is no per-processor grouping.
2. **It's slow.** Even to produce that single bucket, it reads and parses every
   descriptor file. For 10,000 cached products that's 10,000 syscalls and
   10,000 JSON parses, just to count entries.

## Why this matters: declared-but-empty processors

In `analyzers stats`, if a user declares `[analyzer.cpp]` in `rsconstruct.toml`
but the analyzer never matches anything, the table shows a `cpp  0  0` row
(implemented 2026-04-12). This is a useful signal: "you configured it, but it
is silently doing nothing."

We'd like the same in `cache stats`: show every enabled processor, including
those with zero cached entries, so that users notice mis-configurations.

We **cannot** implement this today. If we listed declared processors with
zeros, real entries would still be lumped into `"all"`, so the table would
show:

```
all:    50 entries, 58 outputs, 3.2 MiB
ruff:    0 entries, 0 outputs, 0 bytes      ← misleading
pylint:  0 entries, 0 outputs, 0 bytes      ← misleading
Total:  50 entries, 58 outputs, 3.2 MiB
```

That's **worse** than the current output — it tells the user "pylint produced
nothing" when pylint may actually have plenty. Fixing the 0-rows UX requires
first fixing the grouping itself.

## Options to fix per-processor cache grouping

### Option A — embed the processor name inside each descriptor

Add a `processor: String` field to `CacheDescriptor`. The cache-insert path
populates it (already known at that point). `stats_by_processor` reads the
field instead of hard-coding `"all"`.

- ✅ Small, localized change — ~100–150 lines including a backward-compat
  fallback for old descriptors.
- ❌ **Does not fix the slowness.** We still read and parse every descriptor
  to learn the grouping.
- ❌ Cache format change requires either a migration step, a "legacy entries
  show up as `unknown`" fallback, or a cache wipe on upgrade.

### Option B — encode the processor name in the descriptor's path

Layout changes from:

```
.rsconstruct/cache/descriptors/
    ab/
        cd/
            abcd1234…json
```

to:

```
.rsconstruct/cache/descriptors/
    ruff/
        abcd.json
        ef01.json
    pylint/
        9876.json
```

`stats_by_processor` becomes:

```rust
for each subdir of descriptors/:
    name = subdir.file_name()       // free — already a String in the dir entry
    count = number of files in subdir  // one readdir per processor
```

- ✅ Fixes grouping **and** speed simultaneously. 30 `readdir`s instead of
  10,000 `read`s is two to three orders of magnitude faster.
- ✅ Trivially answers "does this processor have any cached entries at all?"
  with `exists(descriptors/NAME/)`.
- ❌ Changes on-disk cache layout. Requires migration.

Since descriptors are a cache by definition (regenerable from a build), the
simplest migration is: **detect the old layout on startup and wipe it.** Next
build repopulates under the new layout. No data loss beyond a slower first
build post-upgrade.

### Option C — maintain a processor→count index in a redb sidecar

Keep a small redb database (e.g. `.rsconstruct/cache/stats.redb`) with a table
mapping `processor_name → (entry_count, output_count, output_bytes)`. The
cache insert / evict paths update this index transactionally alongside the
descriptor write.

`stats_by_processor` becomes:

```rust
let db = redb::Database::open("cache/stats.redb")?;
let table = db.begin_read()?.open_table(STATS_TABLE)?;
// One DB read per processor — counts are pre-aggregated.
```

- ✅ Answers `cache stats` in O(P) where P = number of processors, independent
  of cache size. Even faster than Option B at scale.
- ✅ No on-disk layout change to the descriptors themselves — the sidecar sits
  alongside the existing directory structure.
- ✅ Bytes / output counts are maintained eagerly, so the "bytes" axis is also
  free (unlike Option B, which still needs to `stat` each blob for bytes).
- ❌ **Two sources of truth.** If the sidecar and the descriptor directory
  ever disagree (crash mid-write, manual `rm` of a descriptor, remote-cache
  sync, a bug in an insert path), the UI lies. Requires either transactional
  atomicity across two stores (hard — redb transaction + filesystem write) or
  a periodic reconciliation pass.
- ❌ Every cache-insert path needs to update the sidecar. Miss one, and the
  counts drift silently. Options B and A put the source-of-truth physically
  next to the cache entry, so there's no drift to manage.
- ❌ Cache invalidation logic gets more complex: evicting a descriptor now
  means "delete the file AND decrement the counter AND handle the decrement
  failing." More moving parts, more places for bugs.
- ❌ Doesn't help with any future "list all entries for processor X" query —
  you'd still need Option B's path layout for that, or fall back to a full
  walk.

**Verdict**: Option C is the fastest for this one specific query, but it pays
for it with a consistency problem that didn't exist before. Options A and B
keep the cache self-describing — the descriptor itself (or its path) IS the
fact — so they're immune to drift.

### Option comparison

| Aspect | A (field in descriptor) | B (processor in path) | C (redb sidecar) |
|---|---|---|---|
| Grouping correctness | yes | yes | yes (if kept in sync) |
| Scan cost | O(N) reads | O(P) readdirs | O(P) DB reads |
| Bytes count free | no | no (still stat blobs) | yes (pre-aggregated) |
| On-disk layout change | descriptor format | directory layout | new sidecar file |
| Source of truth | descriptor | descriptor path | **two stores** |
| Drift risk | none | none | real — needs reconciliation |
| Migration cost | wipe or dual-read | wipe | initial scan to populate |
| Code complexity | low | low | medium-high |
| Helps other queries | no | yes (list-by-processor) | no |

### Recommendation

**Option B.** The extra invasiveness is one-time (migration). The speed and
correctness wins are permanent; the path layout is self-describing, so no
drift risk; and it also unlocks fast "list entries for processor X" queries
that Options A and C don't.

Option C is attractive if the only query we cared about was a single summary,
but the sidecar's consistency burden is real and tends to surface as bugs in
edge cases (remote-cache sync, partial writes, manual cleanup).

## Secondary cleanup — graph-level helpers

Every caller that wants per-processor grouping over the current graph
currently writes the same `HashMap` pattern inline:

```rust
let mut per_processor: HashMap<&str, _> = HashMap::new();
for product in graph.products() {
    per_processor.entry(&product.processor).or_default() += ...;
}
```

We could add `BuildGraph::products_by_processor() -> &HashMap<String, Vec<ProductId>>`
as a lazily-computed cached view (computed on first access, invalidated only
when the graph is mutated).

- Benefit: de-duplicates the pattern in ~5 call sites.
- Cost: caching / invalidation logic.
- Priority: **low.** The inline grouping is O(N) over RAM iteration and is
  not a performance bottleneck.

Don't do this unless a sixth call site shows up.

## Current state (2026-04-12)

- `analyzers stats`: **fixed.** Shows declared-but-empty rows. Separator
  between data and Total.
- `cache stats`: **unchanged.** Still uses single-bucket `"all"` grouping.
  Documented as a known limitation here; fix is pending Option B.
- Graph helpers: **not added.** Inline pattern remains across call sites.

## See also

- [Cache System](cache.md) — object-store layout, descriptor keys.
- [Checksum Cache](checksum-cache.md) — mtime-based content-hash caching.
- [Dependency Caching](dependency-caching.md) — analyzer dependency cache
  (which *does* have per-analyzer grouping built in).
