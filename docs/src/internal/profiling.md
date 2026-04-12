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

```
cargo build --profile profiling
# binary lands in target/profiling/rsconstruct
```

### Prerequisite: relax `perf_event_paranoid`

Kernel sampling (perf, samply) requires `kernel.perf_event_paranoid <= 1`. On
a personal dev machine, persist it:

```
echo 'kernel.perf_event_paranoid = 1' | sudo tee /etc/sysctl.d/60-perf.conf
sudo sysctl --system
```

### Record with `perf` (text-pipeline-friendly)

On CPUs without LBR (most laptops), DWARF unwinding is very slow to
post-process — don't use `--call-graph dwarf` unless you're patient. Without a
call graph you still get reliable self-time attribution:

```
perf record -F 999 -o /tmp/rsc.perf.data -- \
    target/profiling/rsconstruct --quiet --color=never status

perf report -i /tmp/rsc.perf.data --stdio --no-children \
    --sort symbol --percent-limit 0.1
```

### Alternative: `samply` (Firefox-Profiler UI)

```
cargo install samply
samply record -r 4000 -o /tmp/rsc.json.gz -- \
    target/profiling/rsconstruct --quiet --color=never status
```

Default behavior opens a local UI. Use `--save-only` to just write the file.

### Hardware counters

```
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

## See also

- [Per-Processor Statistics](per-processor-stats.md) — the previous perf
  discussion; describes why `cache stats` is slow (O(N descriptor reads)).
  That's independent of this graph-construction finding.
- [Architecture](architecture.md) — overview of the graph and how products
  are added.
