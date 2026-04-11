# Cache System

RSConstruct uses a content-addressed cache to enable fast incremental builds. This page describes the cache architecture, storage format, and rebuild logic.

## Overview

The cache lives in `.rsconstruct/` and consists of:

- `objects/` — content-addressed object store (all cache data)
- `deps.redb` — source file dependency cache (see [Dependency Caching](dependency-caching.md))

There is no separate database. The object store is the cache.

## Data model

The object store contains three kinds of objects, inspired by git:

### Blobs

A blob is a file's raw content, addressed by its SHA-256 content hash. Blobs are optionally zstd-compressed and made read-only to prevent corruption when restored via hardlinks.

Blobs are stored content-addressed — two products producing identical output share the same blob. This enables deduplication and hardlink-based restoration.

### Why blobs don't store output paths

A blob is pure content — it has no knowledge of where it will be restored. This is critical for two reasons:

1. **Rename survival.** If you rename `foo.md` to `bar.md` without changing its content, the cache key (which is content-addressed) is the same. The blob is reused and restored to the new output path (`bar.txt` instead of `foo.txt`). If the blob stored its output path, this wouldn't work.

2. **Deduplication across trees.** Multiple tree entries can point to the same blob under different paths. For example, if two files in a creator's output have identical content, they share the same blob object in the store. The tree records the path; the blob just holds the content.

### Trees

A tree is a serialized list of `(path, mode, blob_checksum)` entries describing a set of output files. Trees are stored in the object store, addressed by the cache key (not by content hash). A tree maps relative file paths to content-addressed blobs. Multiple trees can point to the same blobs — deduplication happens at the blob level.

### Markers

A marker is a zero-byte object indicating that a check passed. Markers are stored in the object store, addressed by the cache key.

### Cache entries

A cache entry is a small descriptor stored in the object store at the path derived from the cache key. It contains:

```json
{"type": "blob", "checksum": "abc123...", "mode": 493}
```

Note: the blob descriptor has no path — the product knows where its output goes.

Or:

```json
{"type": "tree", "entries": [{"path": "dir/file.txt", "checksum": "def456...", "mode": 493}]}
```

or:

```json
{"type": "marker"}
```

The actual file content lives in separate content-addressed blob objects. The cache entry is just a pointer (for generators) or a manifest (for creators).

### Object store layout

```
.rsconstruct/objects/
  a1/b2c3d4...    # could be a blob (raw file content)
  ff/0011aa...    # could be a cache entry (JSON descriptor)
  cd/ef5678...    # could be another blob
```

Cache entries and blobs share the same object store. Cache entries are addressed by cache key hash; blobs are addressed by content hash.

## Cache keys

The cache key identifies a product. It is computed as:

```
hash(processor_name, config_hash, input_content_hash)
```

Where:
- `processor_name` — the processor type (e.g., `pandoc`, `ruff`)
- `config_hash` — hash of the processor configuration (compiler flags, args, etc.)
- `input_content_hash` — combined SHA-256 hash of all input file contents

The key is **content-addressed**: it depends on what the inputs contain, not what they're named. Renaming a file without changing its content produces the same cache key.

### Multi-format processors

For processors that produce multiple output formats from the same input (e.g., pandoc producing PDF, HTML, and DOCX), each format is a separate product with a separate cache key. The output format is part of the config hash, so each format gets its own key naturally.

### Output depends on input name

Most processors produce output that depends only on input content. However, some processors embed the input filename in the output (e.g., a `// Generated from foo.c` header). For these processors, the `output_depends_on_input_name` property is set to `true`, and the input file path is included in the cache key:

```
hash(processor_name, config_hash, input_content_hash, input_path)
```

## Flows

### Lookup

1. Compute the cache key from processor name + config + input contents
2. Look up the object at that key in the object store
3. If not found: cache miss, product must be built
4. If found: read the descriptor, act based on type

### Cache (after successful build)

**Checker:**
1. Store a `{"type": "marker"}` entry at the cache key

**Generator (single output):**
1. Store the output file content as a content-addressed blob
2. Store a `{"type": "blob", "checksum": "..."}` entry at the cache key

**Creator (multiple outputs):**
1. Walk all output directories and files
2. Store each file as a content-addressed blob
3. Build the tree entries: `[{"path": "...", "checksum": "...", "mode": ...}, ...]`
4. Store a `{"type": "tree", "entries": [...]}` entry at the cache key

### Restore

**Checker:** Nothing to restore. Cache entry exists = check passed.

**Generator:**
1. Read the cache entry, get the blob checksum
2. Hardlink or copy the blob to the output path

**Creator:**
1. Read the cache entry, get the tree entries
2. For each `(path, checksum, mode)`: restore the blob to the path, set permissions

### Skip

If the cache entry exists AND all output files are present on disk, no work is needed.

## Rebuild classification

| Classification | Condition | Action |
|---|---|---|
| **Skip** | Cache key found AND all outputs exist on disk | No work needed |
| **Restore** | Cache key found BUT some outputs are missing | Restore from object store |
| **Build** | No cache entry for this key | Execute the processor |

Because the cache key incorporates input content, a changed input produces a different key. There's no "stale entry" — either the key exists or it doesn't.

## Config-aware caching

Processor configuration is hashed into cache keys. Changing a config value triggers rebuilds even if source files haven't changed.

## Cache restoration methods

| Method | Behavior | Best for |
|---|---|---|
| `hardlink` | Links output to cached blob (same inode, read-only) | Local development (fast, no disk space) |
| `copy` | Copies cached blob to output path (writable) | CI runners, cross-filesystem setups |
| `auto` (default) | Uses `copy` when `CI=true`, `hardlink` otherwise | Most setups |

Hardlinks work because blob objects contain raw file content (not wrapped in a descriptor). Only cache entries (which point to blobs) contain JSON metadata.

## Cache commands

| Command | Description |
|---|---|
| `rsconstruct cache size` | Show cache size and object count |
| `rsconstruct cache list` | List all cache entries as JSON |
| `rsconstruct cache stats` | Show per-processor cache statistics |
| `rsconstruct cache trim` | Remove unreferenced objects |
| `rsconstruct cache clear` | Delete the entire cache |

## Clean vs Clear

**`rsconstruct clean`** removes build outputs but preserves the cache:

- **Generators**: Output files deleted. Next build restores via hardlink/copy.
- **Checkers**: Nothing to delete. Next build skips.
- **Creators**: Output directories deleted. Next build restores from tree.

**`rsconstruct cache clear`** wipes everything — descriptors and blobs. A cleared cache means "forget everything, rebuild from scratch." The entire `.rsconstruct/` directory is removed. If only blobs were cleared but descriptors survived, the cache would think outputs are available but fail to restore them. Clearing both together avoids this inconsistency.

## Incremental rebuild after partial failure

Each product is cached independently after successful execution. If a build fails partway through, the next run only rebuilds products without valid cache entries.

## Remote caching

See [Remote Caching](remote-caching.md) for sharing cache between machines and CI.
