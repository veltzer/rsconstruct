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

1. **Numeric/cast lints**: `cast_possible_truncation`, `cast_precision_loss`, `cast_sign_loss`, `cast_possible_wrap`. These fire on progress-percentage computations and similar internal arithmetic where the values are bounded and not user-controlled. Per-site allows would add noise without adding safety.
2. **Stylistic / debatable**: lints where clippy's preferred shape is not obviously better. Examples: `option_if_let_else` (`map_or` form is often less readable), `match_same_arms` (kept distinct for readability), `items_after_statements` (a local `static` regex stays next to the code that uses it).
3. **Mutex guard tightening**: `significant_drop_tightening`. Flags every guard whose scope extends past the last use, even by one statement. In practice our guards are held for short cache lookups and tightening produces busier code without measurable contention reduction. Deliberate policy allow.

There is no "to-do" group remaining: the previous strictness pass either fixed each pedantic/nursery lint in code or moved it to one of the three buckets above with rationale.

## The rule for `#[allow(...)]`

Every `#[allow(...)]` in the codebase MUST:

1. **Be necessary.** If the compiler accepts the code without the allow, remove the allow. The compiler is cleverer than you think — dead code that looks dead to a human is sometimes reachable, and vice versa.
2. **Be scoped minimally.** Attach the allow to the smallest item (a single field, a single function, a single import) that requires it — never to a whole struct or module when one member is the culprit.
3. **Carry a comment explaining why.** The comment answers: "what feature/workflow keeps this thing around despite looking dead?" A silent `#[allow(dead_code)]` is a bug.
4. **Be periodically re-audited.** Scaffolding becomes production code (allow removed) or is abandoned (code deleted). Long-lived allows are a code smell.

## Per-item `#[allow]` attributes

Below the crate-level block, every remaining allow sits on a single item and carries its reason, either as a comment or as the attribute's `reason = "..."`. `grep -rn '#\[allow(' src/` lists them; the set changes as code changes, so this page describes their kinds rather than enumerating them:

- **Clippy lints that are wrong for one call site**, such as `too_many_arguments` on a function that mirrors an upstream signature, `unnecessary_wraps` on a function whose `Result` is fixed by the `PhaseHook::run` signature, or `naive_bytecount` where a dependency for one count is not worth it.
- **Ported engines under `src/engines/`**, which follow their upstream's structure and naming: `non_upper_case_globals` and `non_camel_case_types` on names copied from the original, and `dead_code` on the faithful ASTs and generated tables (`hadolint/parser.rs`, `checkstyle/tables`) whose unread fields still document what the upstream parser accepts.

No `#[allow(dead_code)]` guards scaffolding any more. The remote-pull path that an earlier version of this page listed (the `remote_pull` field and the `try_fetch_*` helpers in `src/object_store/`) is wired into the read path, and `ProcessorPlugin.processor_type` is read by the registry's name lookup (`src/registries/processor.rs`) and by the completeness tests, so both allows were removed.

## What the pass removed

Seven allows were removed during the most recent strictness sweep. Three of them masked genuine dead code, which was then deleted:

- `checksum::invalidate()` — never called; deleted.
- `checksum::clear_cache()` — never called; deleted.
- `ProcessorBase.name` field + `ProcessorBase::auto_detect()` helper — never read, never called; deleted.

Four were stale — the code they guarded was actually used, and the allow no longer made the compiler quieter:

- `remote_cache::RemoteCache::download` — used by `operations.rs`; allow removed.
- `exit_code::IoError` — used in match arms and by the `error` CLI command; allow removed.
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
