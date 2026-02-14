# Rejected Audit Findings

Issues flagged during code audits (rounds 7-12) that were assessed and deliberately rejected. Documented here to prevent re-flagging in future audits.

## Duration u128-to-u64 overflow in JSON output

**File:** `src/json_output.rs` (lines 130, 151)
**Flagged in:** rounds 9, 10, 11, 12

`Duration::as_millis()` returns `u128`, cast to `u64` without bounds checking. Overflows after ~584 million years. No real build will ever hit this. Not fixing.

## Pre-1970 mtime cache collision

**File:** `src/object_store/checksums.rs` (lines 25-27)
**Flagged in:** rounds 9, 10, 11, 12

Files with mtime before Unix epoch (1970) get `unwrap_or_default()` mapping to `(0, 0)`. Two such files could share a cached mtime entry. Pre-1970 timestamps don't occur on real build inputs. The mtime cache is only an optimization — the actual input checksum comparison catches real changes. Not fixing.

## Dependency unchanged logic — no-dep products

**File:** `src/executor/execution.rs` (line 587)
**Flagged in:** round 9

Agent claimed `!deps.is_empty() && deps.iter().all(...)` should be `deps.is_empty() || deps.iter().all(...)`. Wrong — products with no dependencies should NOT reuse cached checksums. The optimization is specifically for products whose upstream deps produced identical output, meaning transitive inputs are unchanged. No-dep products have no such guarantee.

## Batch handle_success return value ignored

**File:** `src/executor/execution.rs` (line 339)
**Flagged in:** round 10

`handle_success()` return value is not checked in batch processing. This is correct — `handle_success` already calls `record_failure` internally when caching fails, properly marking the product as failed. In non-batch, the return value triggers a `break` from the retry loop, but batch has no retry loop. Stats are correct either way.

## record_failure ignores mark_processor_failed in keep-going mode

**File:** `src/executor/handlers.rs` (lines 20-39)
**Flagged in:** rounds 11, 12

In keep-going mode, `mark_processor_failed` parameter is ignored. This is by design — `failed_processors` is only checked in non-keep-going mode to skip subsequent products from the same processor. In keep-going mode, all products run regardless, so tracking failed processors is unnecessary.

## Arc reference leak — failed_processors not unwrapped

**File:** `src/executor/execution.rs` (collect_build_stats)
**Flagged in:** round 11

Agent claimed not unwrapping `failed_processors` Arc prevents other `Arc::try_unwrap` calls from succeeding. Wrong — each Arc has its own independent reference count. Not unwrapping one has zero effect on others.

## Tera output paths lose directory structure

**File:** `src/processors/generators/tera.rs` (lines 100-106)
**Flagged in:** round 10

Templates in subdirectories produce output at project root (e.g., `templates/sub/README.md.tera` → `README.md`). This is intentional — the comment on line 105 explicitly says "Output is at project root with the .tera extension stripped." By design.

## Lua stub_path uses suffix as directory name

**File:** `src/processors/lua_processor.rs` (line 126)
**Flagged in:** round 10

`rsb.stub_path(source, suffix)` uses `suffix` to construct the output directory (`out/{suffix}`). This is the designed Lua API — plugins control their own output directory naming via the suffix parameter.

## Lua clean count masking with saturating_sub

**File:** `src/processors/lua_processor.rs` (lines 450-468)
**Flagged in:** rounds 10, 11

Custom Lua clean functions report removal count via `existed_before.saturating_sub(exist_after)`. If the Lua function doesn't remove files, that's the plugin's responsibility. The count accurately reflects what was actually removed. Not a bug.

## file_index exclude_dirs substring matching

**File:** `src/file_index.rs` (lines 76-80)
**Flagged in:** rounds 9, 10

`exclude_dirs` uses `path_str.contains(dir)` for filtering. The documented convention uses slash-delimited patterns like `"/kernel/"`, which prevents false positives on path substrings. This is the configured behavior.

## Object store trim path reconstruction

**File:** `src/object_store/management.rs` (lines 86-103)
**Flagged in:** round 9

Reconstructing checksums from filesystem paths (`prefix + rest`) during cache trim. The path structure is fixed (`objects/[2-char]/[rest]`), set by `store_object()`. Unexpected files in the objects directory are silently ignored during trim, which is the correct behavior.

## Partial output caching (before the fix)

**File:** `src/object_store/operations.rs` (lines 144-147)
**Flagged in:** round 9

Originally flagged as a design choice. User overruled — missing outputs are now an error (`anyhow::ensure!`). This was **accepted and fixed** in a later commit, not rejected.

## Spellcheck read-modify-write race

**File:** `src/processors/checkers/spellcheck.rs` (lines 192-229)
**Flagged in:** round 11

Agent claimed file read-modify-write isn't protected. Wrong — `self.words_to_add.lock()` on line 193 acquires the mutex, which is held for the entire function (not dropped until return). The lock prevents concurrent threads from interleaving. Cross-process races are not a concern for RSB.

## Duplicate dependency edges in resolve_dependencies

**File:** `src/graph.rs` (lines 227-230)
**Flagged in:** round 12

Agent claimed duplicate edges cause incorrect topological sort. The scenario requires a product to list the same input file twice, which doesn't happen — `FileIndex.scan()` returns unique paths. Even if it did, duplicate edges would increment and decrement `in_degree` the same number of times, netting out correctly.

## Python string injection in load_python_config

**File:** `src/processors/generators/tera.rs` (lines 205-208)
**Flagged in:** round 12

Agent claimed newlines in file paths could inject Python code. File paths come from `FileIndex` (filesystem scan) or Tera templates written by the project author — both are trusted input. Linux file paths from filesystem scans don't contain newlines.

## Batch assert_eq should be error return

**File:** `src/executor/execution.rs` (lines 323-325)
**Flagged in:** round 12

Agent suggested replacing `assert_eq!` with `anyhow::bail!` for batch result count validation. The assert is deliberate — a processor returning the wrong number of results is a contract violation (programming error), not a recoverable runtime condition. Assertions are appropriate for invariant violations.

## Platform portability (Windows, macOS)

**Flagged in:** rounds 9, 10, 11, 12

Multiple agents flagged `std::os::unix` usage without `#[cfg(unix)]` guards, and missing `#[cfg(windows)]`/`#[cfg(target_os = "macos")]` blocks. RSB is Linux-only. No platform compatibility code will be added.

## DB recovery — file might not exist

**File:** `src/db.rs`
**Flagged in:** round 12

Agent re-flagged db.rs recovery, claiming `fs::remove_file` could fail if the file doesn't exist. This was already fixed in round 8 — `let _ = fs::remove_file()` was changed to `fs::remove_file()?` which properly propagates errors.
