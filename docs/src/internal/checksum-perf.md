# Feature design: checksum performance — DONE (streaming)

## Status

Streaming hash (option A below) is implemented. xattr/sidecar (sub-question
1) and mmap (option B/C below) are answered as recommendations not to
pursue at this time.

## Origin

`problems.txt`:

> doing checksum of files. could we store the checksum next to the file
> on disk as extra data? are we using mmap when doing the checksum?
> Maybe we could speed up the checksum calculation this way.

Two sub-questions:

1. Store the checksum next to the file on disk (as extra data / xattr /
   sidecar) so we don't re-hash unchanged files?
2. Use mmap when hashing to speed up the calculation?

## Sub-question 1 — "store the checksum next to the file"

### Already done, just not where the user suggested

The current design (`src/checksum.rs`) already avoids re-hashing
unchanged files. The mechanism:

- `checksum_fast(ctx, path)` first calls `stat()` on the file and reads
  its mtime.
- It looks up `(path → MtimeEntry { mtime, checksum })` in a persistent
  redb database at `.rsconstruct/mtime.redb`.
- If the recorded mtime matches the file's current mtime, the cached
  checksum is returned **without reading the file**.
- If the mtime differs (or no entry exists), the file is read, hashed,
  and the new `(mtime, checksum)` is written back.
- Mtime → checksum is also kept in an in-memory `HashMap` keyed by
  `PathBuf` for the duration of one process, so the second call for the
  same path within one build is free.

This is the same shape as what the user is proposing, with one design
choice different: the mapping lives in **one centralized redb file**
instead of being distributed as per-file metadata.

### Why centralized beats distributed for our case

Three options for "store the checksum near the file":

| Option              | How                                            | Problems                                                                 |
| ------------------- | ---------------------------------------------- | ------------------------------------------------------------------------ |
| **xattrs**          | `setxattr(path, "user.rsconstruct.sha256", ...)` | tmpfs / NFS / sshfs / WSL often don't support them; `cp`, `tar`, `rsync` don't preserve them by default; `mv` across filesystems silently drops them. Result: cache misses are silent and frequent. |
| **Sidecar files**   | `path.sha256` next to `path`                   | Pollutes the tree. Files accidentally get committed. Glob patterns (`*.sha256`) noise up unrelated tools. Dotfile sidecars (`.path.sha256`) have similar issues plus visibility quirks. |
| **Centralized DB**  | One redb at `.rsconstruct/mtime.redb` (current) | Single file, single rule for ignore, easy to nuke, no FS dependencies, survives across builds.                                                |

We picked centralized DB. It works on every filesystem rsconstruct
runs on, doesn't pollute the project tree, and is already
gitignored as part of `.rsconstruct/`.

### The case for sidecars anyway

There's one scenario where xattrs would beat redb: **multiple checkouts
of the same project sharing a workspace** (think Bazel-style build
farms, or developers who clone the same repo into 5 worktrees). With
redb, each clone has its own cache and re-hashes everything once. With
xattrs, the cache lives on the file inode and is shared across clones.

Not worth pursuing for rsconstruct's typical workload. If we ever want
this, the right model is a content-addressed store keyed by `(stat()
ino + dev + mtime)` shared across users, but that's a different feature.

### Recommendation

Document that we already do this, and why we chose centralized over
distributed. Don't change the mechanism. Update the
`docs/src/internal/checksum-cache.md` chapter to be explicit about the
trade-off.

## Sub-question 2 — "are we using mmap?"

### Current implementation

`file_checksum` in `src/checksum.rs:38-49` does:

```rust
let contents = fs::read(path)?;     // allocates Vec<u8> the size of the file
let checksum = hex::encode(Sha256::digest(&contents));
```

Two issues with this for large files:

1. **Memory**: a 100 MB file allocates a 100 MB Vec, then hashes it,
   then drops it. Peak RSS spike per file.
2. **Throughput**: read-into-buffer + hash is two passes over the same
   bytes (kernel→buffer copy, then SHA-256 over the buffer). With mmap
   the kernel maps the pages directly and SHA-256 reads them from the
   page cache without an explicit copy.

### What faster alternatives exist

Three options, increasing in complexity:

**(A) Streaming reads**: open the file, read 64 KB chunks into a fixed
buffer, feed them to `Sha256::update`. No mmap, no large allocation. Code:

```rust
let mut file = fs::File::open(path)?;
let mut hasher = Sha256::new();
let mut buf = [0u8; 65536];
loop {
    let n = file.read(&mut buf)?;
    if n == 0 { break; }
    hasher.update(&buf[..n]);
}
let checksum = hex::encode(hasher.finalize());
```

Wins: bounded memory, no allocation per file. Loss: same number of
syscalls as the current `fs::read` (which does this internally).
Throughput is comparable for small files and much better for large
ones (no Vec growth).

**(B) mmap + hash**: `memmap2::Mmap::map(&file)`, `hasher.update(&mmap)`.

Wins: zero-copy when pages are already in the page cache; potentially
fastest path for large files.

Loss:
- `mmap` page-fault overhead can exceed read overhead for small files.
- A file truncated under us → SIGBUS, which by default kills the
  process. Robust handling needs `sigaction` setup and is a project we
  do not currently want.
- Some filesystems (network, FUSE) handle mmap badly or not at all.
- mmap uses virtual address space; on 32-bit hosts (rare) or with
  thousands of large files this matters.

**(C) Hybrid (size-gated)**: mmap for files above some threshold (say,
4 MB), streaming reads below. Captures the wins of mmap on large
artifacts without the overhead on the typical small-file workload.

### What rsconstruct typically hashes

Sampling teaching-syllabi (representative of the user's workload):

- ~700 .md files, all under 100 KB.
- Generated PDFs in `out/`, mostly < 1 MB.
- The .redb caches are larger but are not themselves hashed.

For this workload the difference between read+hash and mmap+hash is
within the noise. The wall-clock cost of hashing in a clean rebuild is
already dominated by the SHA-256 computation, not by the I/O method.

The only workloads where mmap clearly wins are projects that hash
**large generated binaries** (>10 MB) repeatedly. These exist (CC
processor outputs for big projects, PDFs from large LaTeX runs) but
aren't the norm.

### Recommendation

**Do (A), skip (B) and (C).**

(A) — streaming read — is a strict improvement over the current code:
bounded memory, simpler, fewer allocations, no SIGBUS pitfall, works
everywhere `fs::read` works. The win is small for small files and
solid for large ones. ~30 lines of code, no new dependency, no new
failure modes.

(B) — mmap — is real but the gains for our typical workload are
marginal and the failure modes (SIGBUS, FS support) are real. Reach for
this only if profiling shows hashing is a bottleneck, and even then,
gate it on file size so the mmap is only used when its overhead pays
back.

(C) — hybrid — is the principled answer if (A) isn't enough. Add a new
crate dependency (`memmap2`) for a 5-15% improvement on a workload that
isn't currently a bottleneck. Premature.

### Concrete plan if you say yes

1. Replace `fs::read(path)` in `file_checksum` with a 64 KB streaming
   loop using `Sha256::update`.
2. Same change in `bytes_checksum`? No — that one already takes
   `&[u8]`, no I/O.
3. No API changes. `file_checksum` returns the same string.
4. No changes to the mtime cache — it sits in front of the hash and
   doesn't care how the hash is computed.

Estimated ~30 lines of code in `src/checksum.rs`. No new dependencies.
Probably 0 functional difference for typical projects but the code
becomes simpler (no large-Vec allocation) and large-file hashing
benefits.

## Open questions

1. **Do (A) — streaming hash — now?** Cheap, monotonically better.
   Default yes unless you'd rather wait.

2. **Do (B) or (C) — mmap?** I'd say no until you have a workload that
   demands it. If you do have one (e.g. you regularly hash 50+ MB
   generated artifacts), tell me and I'll do (C).

3. **Document the centralized-vs-distributed choice in the book?** The
   user's question implies they didn't know we already cache. A short
   addition to `docs/src/internal/checksum-cache.md` explaining "why
   not xattr / sidecar" would close the loop and prevent the question
   from coming back.
