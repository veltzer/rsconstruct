# Path Interning

Interning is a data-structure optimization that replaces `PathBuf` HashMap
keys with small integer IDs. It exists to cut the cost of hashing, comparing,
and cloning paths during graph construction.

## Motivation

The [Profiling](profiling.md) run on `teaching-slides` (10,027 products)
pointed at three quadratic scans inside `BuildGraph::add_product_with_variant`.
Replacing those scans with `HashMap<PathBuf, _>` indexes took `status` from
1.08 s to 0.26 s.

The remaining 0.26 s is dominated, by category:

| Category                          | % of CPU |
|-----------------------------------|----------|
| Path iteration (`Components`)     | ~10 %    |
| HashMap hashing (SipHash + Path)  | ~7 %     |
| Allocator churn (malloc/free)     | ~6 %     |
| UTF-8 validation/decoding         | ~7 %     |
| Checksumming (SHA-256 + keys)     | ~6 %     |

A lot of that is the cost of *using `PathBuf` as a HashMap key*. Every insert
and lookup does:

1. **Hash the path** — walks every component, hashes each byte. O(path length).
2. **On collision, compare paths** — walks both paths component-by-component.
3. **Clone the path to store as key** — `PathBuf` allocation + copy.

With ~10,000 products participating in multiple maps (`output_to_product`,
`input_to_products`, `checker_dedup`), this work dominates what remains.

## The idea

Assign each unique path a `u32` ID once, then use the ID everywhere the path
is used as a HashMap key or for comparison. Hashing a `u32` is one
instruction. Comparing two `u32`s is one instruction. No allocation.

```rust
#[derive(Copy, Clone, Eq, PartialEq, Hash)]
pub struct PathId(u32);

pub struct PathInterner {
    to_id: HashMap<PathBuf, u32>,   // used during insertion
    from_id: Vec<Arc<PathBuf>>,     // id -> path (for display / FS ops)
}

impl PathInterner {
    pub fn intern(&mut self, p: &Path) -> PathId { /* ... */ }
    pub fn get(&self, id: PathId) -> &Path { /* ... */ }
}
```

Every hot HashMap that currently keys on `PathBuf` switches to `PathId`.

## In-memory only

**Interned IDs are per-process.** They are assigned fresh at the start of
every `rsconstruct` invocation and dropped when the process exits. They
never touch disk.

| Data                                    | Lives in           | IDs used? |
|-----------------------------------------|--------------------|-----------|
| `BuildGraph` HashMaps                   | RAM, this process  | Yes       |
| On-disk cache (redb descriptors, etc.)  | Disk, persistent   | No        |
| Config files, discovered files          | Disk               | No        |

The path `foo/bar.md` might be `PathId(42)` today and `PathId(17)` tomorrow.
That is fine because nothing persistent ever referred to `42`.

The boundary rule: **`PathId` must not leak into anything persistent.**
Specifically:

- Cache keys on disk (`Product::cache_key`, `descriptor_key`) must keep
  using real paths or content checksums.
- Logs and error messages must print real paths, not IDs.
- Nothing serializes the interner state.

## Why it helps here

- Paths are reused heavily. One `.md` file feeds `markdownlint`, `zspell`,
  `script.check_md`, `marp`. Interning collapses four HashMap key clones
  into one.
- The same path appears as a lookup key in every dedup map during graph
  construction. Each lookup becomes `hash(u32) + compare(u32)` instead of
  walking a path's components.
- Product inputs/outputs can still be stored as `PathBuf` publicly — the
  optimization targets the *HashMap keys*, not the product data itself.
  This keeps the refactor's blast radius small.

## Scope of the change

Narrow scope — only the three hot HashMaps in `BuildGraph`:

- `output_to_product: HashMap<PathBuf, usize>` → `HashMap<PathId, usize>`
- `input_to_products: HashMap<PathBuf, Vec<usize>>` → `HashMap<PathId, Vec<usize>>`
- `checker_dedup: HashMap<(String, PathBuf, Option<String>), usize>` →
  `HashMap<(String, PathId, Option<String>), usize>`

The interner lives on `BuildGraph`. Callers still pass `PathBuf`/`&Path` to
`add_product*` — the interner is a private implementation detail. Public
access to `Product.inputs`/`outputs`/`output_dirs` remains unchanged.

## Non-goals

- **No on-disk format change.** Cache entries keep using real paths.
- **No API change to `Product`.** Inputs and outputs stay as `Vec<PathBuf>`.
- **No plugin-facing change.** Lua processors keep seeing paths.

## Risks

- The interner's own `to_id` map still hashes a `PathBuf` once per unique
  path. Unavoidable — this is the cost of asking "have I seen this path
  before?"
- Every call site that hashes a `&Path` into a `BuildGraph` map now calls
  `interner.intern()` or `interner.get_id()`. Must be careful not to call
  `intern()` (mutating) on read-only paths, or lookups may create spurious
  entries.

## See also

- [Profiling](profiling.md) — the measurement that motivated this.
- [Architecture](architecture.md) — how `BuildGraph` fits into the overall
  design.
