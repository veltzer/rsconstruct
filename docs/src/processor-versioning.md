# Processor Versioning and Cache Invalidation

When a processor's implementation changes in a way that produces different output for the same input, every cached entry it produced becomes potentially stale. This chapter documents the problem, the design alternatives we considered, and the chosen approach.

## The problem

rsconstruct's cache is content-addressed on a key derived from:

- Primary input file checksums
- `dep_inputs` / `dep_auto` file checksums
- `output_config_hash` (the processor's relevant config fields)
- Tool version hash (optional — e.g. `ruff --version` output)

Crucially absent: **the implementation of the processor itself**.

Consider: a user upgrades rsconstruct to a version where the `ruff` wrapper now passes a new flag by default. Inputs haven't changed. Config hasn't changed. Ruff's binary version hasn't changed. But the output is different — the new flag changes behavior.

rsconstruct sees a cache hit on the old descriptor and restores the stale result. The user gets incorrect output from "fresh" caches.

## Design alternatives considered

### Option A: Hash the binary at startup

Compute a SHA of the rsconstruct binary itself at program start. Mix that hash into every product's cache key.

**How it works:** Any change to any part of rsconstruct — processors, core executor, cache code, even comments — invalidates every cache entry.

**Pros:**
- Trivially correct. If any code changed, caches are invalidated.
- Zero developer action.
- No risk of forgotten invalidation.

**Cons:**
- **Massively over-invalidates.** Fixing a typo in a docstring or reformatting the `clean` command wipes every user's cache across every processor.
- Makes iterating on rsconstruct itself painful — developers constantly rebuild everything.
- Version bumps of unrelated dependencies (regex bumps, anyhow bumps) change the binary and also invalidate.

### Option B: Per-file source hash (automatic)

`build.rs` hashes each processor's `.rs` file at compile time. The hash is embedded as a `&'static str` into that processor's plugin entry. Cache key includes this hash.

**How it works:** Modify `src/processors/checkers/ruff.rs`, next build picks up a new hash, ruff's caches invalidate. Other processors are unaffected.

**Pros:**
- Zero developer action — hashes are automatic.
- More precise than Option A — only the changed processor invalidates.
- Never forget to bump.

**Cons:**
- **Too sensitive.** Whitespace changes, comment fixes, rustfmt reformats, renames of private helpers — all invalidate the cache even though behavior is identical.
- **Doesn't catch indirect changes.** If a processor calls shared helpers in `processors/mod.rs` and those change, the processor's file hash hasn't changed but its behavior has. We need to hash transitive dependencies, and Rust doesn't give us an easy way.
- **Non-deterministic sources of churn:** different rustfmt versions produce different hashes for the same intent, CI vs. local editor differences cause spurious invalidation.
- **Signal dilution:** users stop paying attention to "this rebuilt" because it happens even for cosmetic changes. The signal loses meaning.

### Option C: Whole `src/processors/` subtree hash

Hash the entire processors directory at compile time. Any change to anything under `src/processors/` invalidates every processor's cache.

**How it works:** Middle ground between A and B.

**Pros:**
- Catches shared-helper changes automatically (since helpers are in the same subtree).
- Less aggressive than A — core-executor tweaks don't invalidate.

**Cons:**
- Still over-invalidates — a fix to processor X wipes processor Y's cache.
- Still vulnerable to formatting/comment churn.

### Option D: Explicit per-processor version (manual)

Each processor declares a `version: u32` in its plugin entry. The developer bumps it when making a behavior-changing modification. Cache key includes the version.

**How it works:**
```rust
inventory::submit! { ProcessorPlugin {
    name: "ruff",
    version: 1,   // bump when behavior changes
    ...
}}
```

Commit `Processor ruff: change default flags` becomes the same commit as `version: 1 → version: 2`.

**Pros:**
- **Precise.** Only bumps when the developer decides behavior actually changed.
- **Stable.** Reformats, comment edits, renames do not invalidate caches.
- **Auditable.** Every version bump is visible in git history as a deliberate one-line change with its own rationale.
- **Cross-platform deterministic** — a number, not a hash sensitive to file encoding.
- **Signal stays meaningful** — users see a rebuild only when something actually changed.

**Cons:**
- **Relies on developer discipline.** Forgetting to bump after a behavior change leaves stale caches surviving — a silent correctness bug, arguably worse than no invalidation (because it creates a false sense of safety).
- Requires a documented bump rule so the convention is followed.
- Can be mitigated by code review (diffs show version bumps) and optional CI checks (warn when a processor file changes without a version bump).

### Option E: Hybrid — manual version OR automatic hash, whichever is larger

Both fields exist. The cache key includes max(manual version, auto hash). Belt-and-suspenders.

**Pros:** Catches both forgotten bumps and behavior changes.

**Cons:** Complexity. Two systems doing nearly the same thing. Users don't know which one is "the" trigger. Debugging cache misses becomes harder. Loses the "explicit and predictable" property of Option D.

## Decision: Option D (explicit per-processor version)

For a build system that cares about cache **correctness**, deliberate is better than automatic:

1. **Cache stability is a feature.** Users expect their caches to survive a refactor, a `cargo fmt`, a whitespace cleanup. An automatic hash violates this expectation constantly.
2. **A version bump documents intent.** `git blame` on the `version:` line shows why behavior changed. An auto hash leaves no such record.
3. **The discipline cost is low.** Each behavior-changing commit already requires care — adding a one-line version bump to that care is trivial. Forgetting to bump is caught by code review, same as forgetting a changelog entry or a test.
4. **The discipline failure mode is recoverable.** Worst case: a version bump is forgotten, users report stale caches, we bump the version retroactively in the next release. This is better than the Option B failure mode (constant spurious invalidation drives users to distrust the system).

### The bump rule

Bump a processor's `version` when ANY of:

- The processor would produce different output files for the same inputs.
- The processor would include different content in an output file for the same inputs.
- The processor changes which inputs are discovered (e.g. a new glob pattern, a changed default).
- The processor changes which paths are declared as outputs.
- The processor's interpretation of a config field changes (e.g. what a flag means, how a default is resolved).

Do NOT bump for:

- Refactors with identical behavior.
- Comment / docstring changes.
- Reformatting.
- Renaming of internal helpers.
- Performance improvements that don't change output.
- Bug fixes in error messages (but DO bump if the fix changes which inputs succeed/fail).

When in doubt, bump. A bump is cheap (rebuild all products of one processor once); a missed bump is a correctness bug.

## Implementation outline

1. Add a required `version: u32` field to `ProcessorPlugin` (no default — every processor must declare it).
2. Include the version in the cache key via `output_config_hash` or `descriptor_key`.
3. Initialize all existing processors to `version: 1`.
4. Document the bump rule in a prominent comment near the field definition.
5. (Optional, future) CI check: if a processor file's git diff touches logic but not the `version:` line, post a warning comment on the PR.

## Migration

On the first release after this change ships, every existing cache entry is invalidated (the cache key schema changed). This is a one-time cost, same as any cache-key schema evolution. Users will see a full rebuild once, then cache behavior resumes normally.

## See also

- [Cache System](cache.md) — how the cache is organized and keyed
- [Checksum Cache](checksum-cache.md) — the mtime-based content checksum layer
- [Processor Contract](processor-contract.md) — the broader contract each processor must uphold
