# Profiling

This chapter records concrete profiling runs on rsconstruct, with methodology
and findings pinned to a specific version. Add new runs as new sections with
date + version headers so historical data stays intact.

## How to profile locally

### Build a profile-friendly binary

The default `release` profile strips symbols, so stack traces come out as raw
addresses. `Cargo.toml` defines a `profiling` profile that inherits `release`
but keeps full debug info:

```toml
[profile.profiling]
inherits = "release"
strip = false
debug = true
```

Build with:

```text
cargo build --profile profiling
# binary lands in target/profiling/rsconstruct
```

### Prerequisite: relax `perf_event_paranoid`

Kernel sampling (perf, samply) requires `kernel.perf_event_paranoid <= 1`. On
a personal dev machine, persist it:

```text
echo 'kernel.perf_event_paranoid = 1' | sudo tee /etc/sysctl.d/60-perf.conf
sudo sysctl --system
```

### Record with `perf` (text-pipeline-friendly)

On CPUs without LBR (most laptops), DWARF unwinding is very slow to
post-process — don't use `--call-graph dwarf` unless you're patient. Without a
call graph you still get reliable self-time attribution:

```text
perf record -F 999 -o /tmp/rsc.perf.data -- \
    target/profiling/rsconstruct --quiet --color=never status

perf report -i /tmp/rsc.perf.data --stdio --no-children \
    --sort symbol --percent-limit 0.1
```

### Alternative: `samply` (Firefox-Profiler UI)

```text
cargo install samply
samply record -r 4000 -o /tmp/rsc.json.gz -- \
    target/profiling/rsconstruct --quiet --color=never status
```

Default behavior opens a local UI. Use `--save-only` to just write the file.

### Hardware counters

```text
perf stat -d -- target/profiling/rsconstruct --quiet --color=never status
```

Gives IPC, cache miss rates, branch miss rates — useful for "is this
CPU-bound, memory-bound, or branch-mispredict-bound."

## Run: 2026-04-12 — rsconstruct 0.8.1 — `status` on `teaching-slides`

### Target

- Command: `rsconstruct --quiet --color=never status`
- Project: `../teaching-slides` (10,027 products across 10 processors).
- Product breakdown: explicit (1), ipdfunite (55), markdownlint (824),
  marp (824), ruff (19), script.check_md (824), script.check_svg (3327),
  svglint (3327), tera (2), zspell (824).

### Methodology

- Binary: `target/profiling/rsconstruct` (release + debug info).
- Sampler: `perf record -F 999` (no call-graph — LBR unavailable, DWARF too
  slow to post-process on this host).
- Counters: `perf stat -d`.

### Wall-clock and counters

| Metric | Value |
|---|---|
| Wall time | 1.08 s |
| User time | 0.99 s |
| System time | 0.08 s |
| CPU utilization | 98.7 % of 1 core |
| RSS peak | 28 MB |
| Instructions | 21.10 B |
| Cycles | 5.30 B |
| **IPC** | **3.98** (very high) |
| Frontend stall | 12.8 % |
| Branches | 5.11 B |
| Branch miss rate | 0.60 % |
| L1-dcache loads | 7.03 B |
| L1-dcache miss rate | 4.13 % |

Interpretation: high IPC, low miss rates, low branch mispredictions. The
CPU pipeline is fully utilized — slowness comes from **doing too many
instructions**, not from cache thrash or branch mispredicts.

### Hot spots (self-time)

| % of CPU | Function |
|---|---|
| **48.79 %** | `std::path::Components::parse_next_component_back` |
| **12.90 %** | `<std::path::Components as DoubleEndedIterator>::next_back` |
| **10.84 %** | `rsconstruct::graph::BuildGraph::add_product_with_variant` |
| **8.43 %** | `<std::path::Components as PartialEq>::eq` |
| 1.41 % | `__memcmp_evex_movbe` |
| 1.04 % | `core::str::converts::from_utf8` |
| 0.89 % | `_int_malloc` |
| 0.78 % | `std::fs::DirEntry::file_type` |
| 0.61 % | `<std::path::Path as Hash>::hash` |
| 0.60 % | `<std::path::Components as Iterator>::next` |
| 0.38 % | `std::sys::fs::metadata` |
| 0.38 % | `<sip::Hasher as Hasher>::write` |
| 0.37 % | `sha2::sha256::x86::digest_blocks` |
| 0.34 % | `<core::str::lossy::Utf8Chunks as Iterator>::next` |
| 0.31 % | `_int_realloc` |
| 0.29 % | `_int_free_chunk` |
| 0.19 % | `rsconstruct::graph::Product::cache_key` |
| 0.19 % | `std::path::compare_components` |
| 0.19 % | `serde_json::read::SliceRead::parse_str` |
| 0.19 % | `statx` |
| 0.19 % | `malloc` |
| 0.19 % | `cfree` |
| 0.18 % | `core::hash::BuildHasher::hash_one` |
| rest | scattered < 0.15 % each |

### Findings

**~70 % of CPU is in `PathBuf` iteration / comparison.** Specifically
`parse_next_component_back` + `next_back` + `Components::eq`, all invoked
from `PathBuf` equality and hashing. Filesystem I/O (readdir, stat, open)
is under 2 %. Hashing (SHA-256 + SipHash) is under 1 %.

**The callsite is `BuildGraph::add_product_with_variant`** in `src/graph.rs`
(lines 221–307). It contains three loops whose path-equality cost
dominates the whole run:

- **Lines 232–242 — checker dedup loop.** For every checker product
  (outputs empty), scans every existing product and compares
  `existing.inputs[0] == inputs[0]` (full `PathBuf` equality, which iterates
  components). With 7,000+ checker products in teaching-slides
  (`script.check_md` + `script.check_svg` + `svglint` + `markdownlint` +
  `zspell`), this is an O(P²) pass per processor over the course of
  discovery.

- **Lines 252–253 — superset check for generator re-declarations.**
  Includes `existing.inputs.iter().all(|i| inputs.contains(i))` — an O(M²)
  call, again per-insertion, again comparing `PathBuf`s component-by-component.

- **Lines 246–285 — output conflict check.** Fast path (HashMap lookup);
  not the bottleneck.

Graph mutation itself (`add_product_with_variant` self-time, 10.84 %) is
modest. The quadratic scans inside it are where the time goes — they just
happen to be attributed to the stdlib path-iteration functions.

### Suggested fix (not yet implemented)

Index the checker-dedup and generator-superset lookups via a HashMap keyed on
`(processor, primary_input, variant)` so the linear scans become O(1). For
10,027 products, the expected improvement is ~3×–5× on `status` wall time.

**Scope:** additions to `BuildGraph` (two new HashMap index fields, kept in
sync with `add_product_*`), a small change to `add_product_with_variant` to
do HashMap lookups instead of linear scans. No cache-layout or
on-disk-format changes.

### Raw data

- `/tmp/rsc.perf.data` was recorded and analyzed to produce the tables
  above. Removed afterwards — regenerate via the methodology section if
  needed.

## Run: 2026-04-12 (later) — HEAD after HashMap dedup fix

### Wall-clock and counters

| Metric | Value | vs. 0.8.1 tag |
|---|---|---|
| Wall time | 0.265 s | **4.1× faster** |
| Instructions | 2.05 B | -90 % |
| Cycles | 0.88 B | -83 % |
| IPC | 2.34 | was 3.98 |
| L1-dcache miss rate | 1.34 % | was 4.13 % |

The quadratic path-equality peak is gone. What remains is the normal cost
of using `PathBuf` as HashMap keys.

### Hot spots (self-time, user-space, 9,948 samples, 10 iterations)

| % | Function | Category |
|---|---|---|
| 5.42 | `core::str::converts::from_utf8` | UTF-8 validation |
| 3.52 | `sip::Hasher::write` | HashMap hashing |
| 3.51 | `<Path as Hash>::hash` | HashMap hashing |
| 3.39 | `sha2::sha256::digest_blocks` | Checksumming |
| 2.09 | `Components::next` | Path iteration |
| 2.00 | `_int_malloc` | Allocator |
| 1.92 | `parse_next_component_back` | Path iteration |
| 1.60 | `compare_components` | Path comparison |
| 1.19 | `combined_input_checksum` | Checksumming |
| 1.12 | `Product::cache_key` | Cache keys |

## Run: 2026-04-12 (later still) — HEAD after path interning

### Context

`BuildGraph`'s three hot HashMaps (`output_to_product`, `input_to_products`,
`checker_dedup`) switched from `PathBuf` keys to a private `PathId(u32)`
backed by an in-memory `PathInterner`. See
[Path Interning](path-interning.md) for design.

### Wall-clock and counters

| Metric | Value | vs. previous |
|---|---|---|
| Wall time | 0.245 s | -8 % |
| Instructions | 2.04 B | ~flat |
| Cycles | 0.91 B | ~flat |

### Hot spots (self-time, user-space, 9,925 samples, 10 iterations)

| % | Function | Notes |
|---|---|---|
| 4.34 | `core::str::converts::from_utf8` | unchanged |
| 3.21 | `sha2::sha256::digest_blocks` | unchanged |
| 2.60 | `Components::next` | unchanged |
| **2.39** | `sip::Hasher::write` | **down from 3.52 %** |
| **2.25** | `<Path as Hash>::hash` | **down from 3.51 %** |
| 2.06 | `_int_malloc` | unchanged |
| 1.52 | `resolve_dependencies` | new — attribution shift |
| **1.19** | `compare_components` | **down from 1.60 %** |
| 1.09 | `combined_input_checksum` | unchanged |

Interning paid off exactly where predicted — the hashing/compare columns
dropped, and `resolve_dependencies` appears because its inner loop is now
small enough to self-attribute rather than vanish inside the stdlib path
functions. The total gain is modest (~8 %) because after the HashMap dedup
fix, HashMap key cost was only ~7 % of total, and interning cuts that in
half.

### Candidate next targets (not yet implemented)

1. **UTF-8 validation** (~6 %) — from `display().to_string()` in cache-key
   building. Cache the string form per product or build keys from raw bytes.
2. **`Product::cache_key` + hex encoding** (~2 % combined) — precompute and
   memoize per product.
3. **SHA-256** (~3 %) — already hardware-accelerated; the only lever is
   fewer calls, via memoized `input_checksum` or better batch reuse.

## Run: 2026-11-21 — rsconstruct 0.9.195 — no-op `build` on `teaching-slides`

### Target

- Command: `rsconstruct --quiet --color=never build`, nothing to do.
- Project: `../teaching-slides`, now 18,018 products and 74,705 input
  references (51,187 unique inputs).
- Measured with `/usr/bin/time`, `--trace`, `strace -c` and `perf record -g`
  on a release build with line tables and frame pointers. Note: the
  `profiling` profile (fat LTO plus full debug info) peaked at 8 GB and was
  OOM-killed on a 30 GB desktop; build a thin-LTO release with
  `CARGO_PROFILE_RELEASE_STRIP=false` and
  `CARGO_PROFILE_RELEASE_DEBUG=line-tables-only` instead.

### Before

0.76 s wall (2.3 s with a cold page cache), more system than user time:
381k `statx` calls (about five per input) and 39k files opened. Classify was
236 ms of it, on one thread; the dependency phase 105 ms.

### Changes

1. **Classify runs in parallel**, one dependency level at a time
   (`executor::classify_products`). Products of one level do not depend on
   each other, so "will a dependency change" is answered from finished
   levels and the policy is consulted for exactly the products the serial
   walk consulted it for.
2. **The mtime table is read into memory once** (`checksum::MtimeState`),
   behind a read-write lock. Each lookup used to open a redb read
   transaction under one global mutex, which also serialized any threads
   that checksum.
3. **The dependency phase validates each cached source once.** A
   prediction pass used to validate every entry (source plus every
   dependency) only to print a "to rescan" count, then the scan validated
   them all again.
4. **One `stat` per input** in `combined_input_checksum` instead of
   `exists()` followed by `metadata()`.
5. **Skipped products are settled by the coordinator** instead of being
   queued to workers. Once classify got fast, 16 workers handling 18k
   trivial skip units spent their time contending for the allocator lock
   and the shared stats locks (perf: `futex_wait` under `free`).

### After

| Threads | 0.9.195 | after |
|---|---|---|
| 1 | 0.757 s | 0.530 s |
| 4 | 0.773 s | 0.453 s |
| 16 | 0.790 s | 0.447 s |

`statx` calls fell from 381k to 224k; classify from 236 ms to about 50 ms.

### Candidate next targets

- **Descriptors are 18k small JSON files**, one opened and parsed per
  product on every build, twice (classify, then the executor's re-check).
  This is most of the cold-cache time. Keeping them in redb, or a
  per-product "last good" record, would remove the opens.
- **The executor re-checks every product serially** (`prepare_level_work`):
  it re-takes every input checksum and re-reads every descriptor that
  classify just did, about 100 ms here. The re-check is needed for products
  downstream of something that ran; a product whose dependencies all
  skipped could reuse its classify result.

## Run: 2026-11-22 — one `stat` per input per build

### Target

No-op `build` of a synthetic project: 20,000 JSON files, each checked by
`processor.checker.ijsonlint` (one input per product, no shared inputs),
warm page cache. Compared against the installed 0.9.197.

### Change

Each input's checksum is taken once per build run and then served from
the session map without a `stat` (`checksum::session_checksum`). Writes by
the build evict what they wrote (`forget_in_session`, now also covering
output directories). Before classify, every distinct input is checked on
`parallel` threads (`checksum::fill_session`). See [Checksum
Cache](checksum-cache.md).

### After

| | 0.9.197 | after |
|---|---|---|
| `statx` calls | 40,796 | 20,796 |
| `-j1` wall | 0.167 s | 0.152 s |
| `-j16` wall | 0.134 s | 0.130 s |

The remaining `statx` calls are one per input plus about 800 others. The
benchmark has no shared inputs, so it shows only the executor's second
look disappearing. A project like `teaching-slides` (75k input references
to 51k files) also loses the repeats across products. The win should be
largest with a cold page cache, which was not measured (dropping it needs
root).

## See also

- [Path Interning](path-interning.md) — the optimization applied in the
  most recent run.
- [Per-Processor Statistics](per-processor-stats.md) — the previous perf
  discussion; describes why `cache stats` is slow (O(N descriptor reads)).
  That's independent of this graph-construction finding.
- [Architecture](architecture.md) — overview of the graph and how products
  are added.
- [Fast `stat` on Linux](fast-stat.md) — background on batching `statx`
  via io_uring; relevant if the per-input `fs::metadata` call ever climbs
  out of the noise (currently ~0.19% of CPU).
