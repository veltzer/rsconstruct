# Checksum Cache

RSConstruct uses a centralized checksum system (`src/checksum.rs`) for all file hashing. It has two layers of caching to avoid redundant I/O and computation.

## Architecture

All file checksum operations go through a single entry point: `checksum::file_checksum(path)`. This function never computes the same hash twice.

### Layer 1: In-memory cache (per build run)

A `HashMap<PathBuf, String>` on the `BuildContext` (`checksum_cache`, behind a read-write lock) stores checksums taken during the current build. Once an input's checksum is in it, every later request for that input is answered from the map **without a `stat`** (`checksum::session_checksum`). Content is assumed stable for one run except where the build writes, and every such write evicts what it wrote: the executor calls `forget_in_session` with the product's declared outputs and its output directories (every file under them) whenever it builds, restores or fails a product.

This covers a file shared by many products (a header, a content tree) and the two looks the build takes at every product: classify, then the executor's check just before running it. On a 20k-file benchmark it halved the `statx` calls of a build that changes nothing, down to one per file.

Before classify, `checksum::fill_session` takes the checksum of every distinct input of the graph on several threads (`[build] parallel`), so classify itself only reads the map. Missing files are not remembered (they are usually upstream outputs not written yet), and an unreadable file is left for whoever asks next to report.

One reader must not trust the map: after a product ran, `handle_success` re-checks its inputs to catch an edit made while the tool ran (see `architecture-observations.md`, R2). An edit by the user never reaches `forget_in_session`, so that check calls `fresh_combined_input_checksum`, which stats and looks at every input afresh and bypasses the combined-input memo too. Output files are never served from the map either (`checksum_output`).

The in-memory cache lives for one build run. `rsconstruct watch`, which reuses a context across rebuilds, clears it before each one (`BuildContext::clear_session_caches`).

### Layer 2: Mtime database (across builds)

A persistent redb database at `.rsconstruct/mtime.redb` maps file paths to `(stamp, checksum)` pairs, where the stamp is the file's mtime, ctime, size and inode (`platform::FileStamp`). Before reading a file to compute its checksum, the system checks:

1. Has this file been checksummed in a previous build?
2. Has any part of its stamp changed since then?

If the stamp matches, the cached checksum is returned without reading the file. This avoids I/O for files that haven't been modified between builds — the common case in incremental builds where most files are unchanged.

When the stamp differs (file was modified), the file is read, the new checksum is computed, and both the in-memory cache and the mtime database are updated.

mtime alone is not enough: `cp -p`, `tar x` and `rsync -a` write new content and then set mtime back, which used to leave the old checksum valid and the build skipping with stale outputs. ctime cannot be set from user space and moves on every write and every mtime change, so the stamp catches those; size and inode catch most rewrites and replacements cheaply. This is the same set git's index compares.

No entry is stored for a file whose mtime *or* ctime is less than two seconds old: on a filesystem with coarse timestamps, a second write in the same tick after hashing would otherwise leave a valid-looking entry for stale content. Using ctime as well matters for `cp -p`, whose freshly written file carries an old mtime.

Dirty mtime entries are flushed to the database in a single batch transaction at the end of each checksum computation pass, minimizing database writes.

### Why two layers

| Layer | Scope | Avoids | Cost |
|---|---|---|---|
| In-memory cache | Single build run | Re-reading + re-hashing the same file | HashMap lookup |
| Mtime database | Across builds | Reading unchanged files from disk | `stat()` + DB lookup |

For the first build, every file must be read and hashed. The mtime database is populated as a side effect. On subsequent builds, most files are unchanged — the mtime check skips reading them entirely, and the in-memory cache prevents redundant lookups within the run.

## Configuration

The persistent mtime database can be disabled via `rsconstruct.toml`:

```toml
[cache]
mtime_check = false
```

Or via the command-line flag:

```bash
rsconstruct build --no-mtime-cache
```

When disabled, every file is read and hashed on every build. The in-memory cache still prevents redundant reads within a single run, but there is no cross-build benefit.

**When to disable:** In CI/CD environments with a fresh checkout, the mtime database has nothing cached from previous builds and just adds write overhead. The in-memory cache is sufficient. Use `--no-mtime-cache` (or `mtime_check = false` in config) to skip the database entirely.

The `rsconstruct status` command also disables mtime checking internally to ensure accurate classification.

## Database location

The mtime database is stored at `.rsconstruct/mtime.redb`, separate from the build cache (`objects/` and `db.redb`) and the config tracking database. This separation means:

- `rsconstruct cache clear` removes the entire `.rsconstruct/` directory — build cache AND mtime database (see `cache.md`; the clear must work even when a database is corrupted, so it deletes the whole state dir)
- The mtime database can be deleted independently without affecting cached build outputs

## Combined input checksum

The `combined_input_checksum(inputs)` function computes a single hash representing all input files for a product. It:

1. Checksums each input file (using the two-layer cache)
2. Hashes the checksums as length-prefixed parts (not a separator join —
   see `hash_checksums`, which has an injection-proofing test)

Missing files get a `MISSING:<path>` sentinel so that different sets of missing files produce different combined checksums.

## See also

- [Fast `stat` on Linux](fast-stat.md) — notes on batching the per-input
  `fs::metadata` call (relevant for very large projects, cold cache, or
  network filesystems).
