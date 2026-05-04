# Strictness

This project holds itself to a strict compiler baseline and treats every relaxation as a deliberate, documented choice. This chapter explains the baseline, the rules for opting out, and the history of the most recent strictness pass.

## Crate-level baseline

`src/main.rs` starts with:

```rust
#![deny(clippy::all)]
#![warn(clippy::pedantic)]
#![warn(clippy::nursery)]
#![deny(warnings)]
```

Effect:

- **Every warning is a compile error.** Unused imports, dead code, unused variables, deprecated APIs — all stop the build. There is no "warning fatigue" because there are no warnings.
- **All of Clippy's default lint group (`clippy::all`) is enforced at `deny` level.** This covers ~500 lints spanning correctness, complexity, style, and perf.
- **`clippy::pedantic` and `clippy::nursery` are enforced at `warn` level.** Combined with `#![deny(warnings)]`, this means any pedantic or nursery hit also stops the build, *unless* it is one of the named lints in the per-lint allow block at the top of `src/main.rs`. New code that triggers a pedantic/nursery lint not in that block will fail to compile.

This is one step short of `forbid`. `forbid` cannot be overridden per-item; `deny` allows a per-item `#[allow(...)]` escape hatch. We chose `deny` so that *principled exceptions* remain possible, but each one is an obvious, grep-able act.

## Per-lint allow block

The crate root carries a block of `#![allow(clippy::*)]` attributes — these are pedantic and nursery lints that the codebase deliberately permits. Each falls into one of three groups (the source comments tag them):

1. **Numeric/cast lints** (3 lints). `cast_possible_truncation`, `cast_precision_loss`, `cast_sign_loss`. These fire on progress-percentage computations and similar internal arithmetic where the values are bounded and not user-controlled. Per-site allows would add noise without adding safety.
2. **Stylistic / debatable** (~32 lints). Lints where clippy's preferred shape is not obviously better. Examples: `option_if_let_else` (`map_or` form is often less readable), `match_same_arms` (kept distinct for readability), `doc_markdown` (over-eager about backticking ordinary words), `naive_bytecount` (would require adding the `bytecount` crate for one call site).
3. **Mutex guard tightening** (1 lint). `significant_drop_tightening`. Flags every guard whose scope extends past the last use, even by one statement. In practice our guards are held for short cache lookups and tightening produces busier code without measurable contention reduction. Deliberate policy allow.

There is no "to-do" group remaining: the previous strictness pass either fixed each pedantic/nursery lint in code or moved it to one of the three buckets above with rationale.

## The rule for `#[allow(...)]`

Every `#[allow(...)]` in the codebase MUST:

1. **Be necessary.** If the compiler accepts the code without the allow, remove the allow. The compiler is cleverer than you think — dead code that looks dead to a human is sometimes reachable, and vice versa.
2. **Be scoped minimally.** Attach the allow to the smallest item (a single field, a single function, a single import) that requires it — never to a whole struct or module when one member is the culprit.
3. **Carry a comment explaining why.** The comment answers: "what feature/workflow keeps this thing around despite looking dead?" A silent `#[allow(dead_code)]` is a bug.
4. **Be periodically re-audited.** Scaffolding becomes production code (allow removed) or is abandoned (code deleted). Long-lived allows are a code smell.

## Current `#[allow]` attributes (at time of writing)

After the strictness pass, 5 allows remain. Each is documented in the source and reproduced here with rationale.

### `src/object_store/mod.rs` — `remote_pull` field

```rust
/// Whether to pull from remote cache.
/// Wired into the constructor but not yet consulted by any read path —
/// remote-pull integration is scaffolded in `operations.rs` (the
/// `try_fetch_*` helpers) but not yet called from the executor.
#[allow(dead_code)]
remote_pull: bool,
```

**Why kept**: remote-pull is a real, partially-implemented feature. The `try_fetch_*` helpers exist; they're just not wired into `classify_products` / the restore path yet. Removing the field now would mean re-adding it when we wire up the feature. Keeping it with a comment documents what's missing.

**When to remove**: when remote-pull read paths are wired up, or when we formally abandon remote-pull.

### `src/object_store/operations.rs` — three `try_fetch_*` / `try_push_descriptor_*` helpers

```rust
// Scaffolding for remote-pull: wired into the API surface but not yet
// called from any read path. Intentional; tracked under remote-pull WIP.
#[allow(dead_code)]
pub(super) fn try_fetch_object_from_remote(&self, checksum: &str) -> Result<bool> { ... }

// Scaffolding for remote-pull (for paired fetch-after-push semantics).
// Not yet called from any write path; tracked under remote-pull WIP.
#[allow(dead_code)]
pub(super) fn try_push_descriptor_to_remote(&self, descriptor_key: &str, data: &[u8]) -> Result<()> { ... }

/// Try to fetch a descriptor from remote cache.
/// Scaffolding for remote-pull; not yet called from any read path.
#[allow(dead_code)]
pub(super) fn try_fetch_descriptor_from_remote(&self, descriptor_key: &str) -> Result<Option<Vec<u8>>> { ... }
```

**Why kept**: same feature as above. These are the building blocks the eventual remote-pull implementation will call. They're tested (implicitly via the types that check they compile), and they work when called — they just aren't called yet.

**When to remove**: same trigger as the `remote_pull` field.

### `src/registries/processor.rs` — `ProcessorPlugin.processor_type` field

```rust
pub struct ProcessorPlugin {
    pub name: &'static str,
    /// Processor type. Declared by every plugin but not yet queried by any
    /// runtime code path — kept as plugin metadata so future features
    /// (e.g. `processors list --type=checker`) can filter without touching
    /// every registration.
    #[allow(dead_code)]
    pub processor_type: ProcessorType,
    ...
}
```

**Why kept**: Every `inventory::submit!` for a processor declares a type (`Checker`, `Generator`, `Creator`, `Explicit`). The runtime currently reads `processor_type()` from the `Processor` trait, never from the plugin. But the static plugin metadata is the right place for filtering features like `rsconstruct processors list --type=checker`. Removing the field now would mean adding 93 `processor_type: ...` lines back later when we want the filter.

**When to remove**: never, once the first feature queries it. Until then, the allow is the cheap price of preserving optionality.

## What the pass removed

Seven allows were removed during the most recent strictness sweep. Three of them masked genuine dead code, which was then deleted:

- `checksum::invalidate()` — never called; deleted.
- `checksum::clear_cache()` — never called; deleted.
- `ProcessorBase.name` field + `ProcessorBase::auto_detect()` helper — never read, never called; deleted.

Four were stale — the code they guarded was actually used, and the allow no longer made the compiler quieter:

- `remote_cache::RemoteCache::download` — used by `operations.rs`; allow removed.
- `exit_code::IoError` — used in match arms and by the `errors` CLI command; allow removed.
- `ProcessorPlugin` struct-level `#[allow(dead_code)]` — only the `processor_type` field needed it; scoped down.
- `builder/mod.rs` — `#[allow(unused_imports)]` on `use crate::config::*;` — the compiler wasn't flagging the glob at all; allow removed.

## What this pass did NOT change

The sweep was focused on `#[allow]` attributes. Broader strictness knobs were left as-is, by choice:

- **`.unwrap()` and `.expect()` counts.** Many are on internal invariants where panicking is correct (contract violation, not user error). An audit could tighten some to `?`, but this is a separate pass with its own judgment calls.
- **`missing_docs`, `missing_debug_implementations`**, etc. Enabling these would require documenting every public item — a much larger change.
- **`clippy::cargo`**. 23 warnings, all about transitive dependency duplication driven by upstream Windows target crates. Not actionable from this repo.
- **The `use crate::config::*;` glob import in `builder/mod.rs`**. Narrowing it would require enumerating ~15 symbols and risks churn. Left as-is.

## Subsequent pedantic/nursery sweep

A later pass (see `doc/strictness-pass.md`) enabled `clippy::pedantic` and `clippy::nursery` at `warn` level. That sweep autofixed about 830 occurrences across ~13 lints (notably `uninlined_format_args`, `redundant_pub_crate`, `redundant_closure_for_method_calls`, `missing_const_for_fn`) and added ~45 explicit per-lint allows for the remainder. See the per-lint allow block in `src/main.rs` and the design doc for the breakdown.

## Adding a new `#[allow]`

When you find yourself wanting to add an `#[allow(...)]`, follow this checklist:

1. **Can the compiler complaint be fixed instead?** Remove the unused import, inline the unused function, prove the variable is live. Most of the time the answer is yes.
2. **Is this the minimum scope?** Put the allow on the single field, not the whole struct. On the single function, not the whole impl. On the single import, not the whole `use` block.
3. **Did you write a comment?** One sentence answering "what feature / workflow justifies this?" is enough. "Reserved for future use" is NOT enough — say what future use, and what would trigger the deletion.
4. **Did you open a tracking concern?** If the allow is for WIP scaffolding, the WIP should be tracked somewhere (a TODO comment with a `// wip:` tag, an issue, a feature flag) so future maintainers know it's temporary.

A reviewer who sees a new `#[allow]` should read the comment, check the rationale, and ask "could we just fix this instead?" before approving.

## Running the audit

A quick sweep to find all current allows:

```bash
grep -rn '#\[allow(' src/
```

For each hit, read the surrounding context and the comment. If the comment is missing or weak, or the code it guards has become truly used, the allow should come out.

## See also

- [Coding Standards](coding-standards.md) — the style rules beyond strictness.
- [Processor Contract](processor-contract.md) — the invariants each processor must uphold.
- `src/main.rs` — the crate-level `#![deny(...)]` directives.
