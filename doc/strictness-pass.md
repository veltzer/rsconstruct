# Feature design: another strictness pass

## Status

Mostly done already. This doc reports what's already strict, what's
deliberately not strict, and what remaining knobs could be turned if you
want to push further.

## Origin

`problems.txt`: "do a pass of making the code more strict (whenever we relax
strictness try to figure out if we can return the strictness)".

## Where the codebase already is

The strictness story is documented in `docs/src/internal/strictness.md`,
which describes a recent strictness sweep. The current state is:

- `#![deny(warnings)]` and `#![deny(clippy::all)]` at the crate root
  (`src/main.rs:1-2`).
- Only **6** `#[allow(...)]` attributes survive in `src/`. Each one is
  named, scoped to the smallest possible item, and accompanied by a
  comment explaining why. Five of those six are scaffolding for the
  partially-implemented remote-pull feature (the field, three helpers,
  and a struct field). The sixth is `clippy::too_many_arguments` on a
  legitimately argument-heavy function in `add_config.rs`.
- `cargo clippy -- -W clippy::pedantic` produces **3 warnings**, all in
  `build.rs`. Source code under `src/` is clean against pedantic.
- `cargo clippy -- -W clippy::nursery` produces **0 warnings**.
- `cargo clippy -- -W clippy::cargo` produces 23 warnings, all of which
  are upstream-dep duplication (mostly Windows target crates) plus two
  missing-metadata fields (`categories`, `keywords`). None are code
  quality.

This is, candidly, much stricter than most Rust codebases. Asking for
"another strictness pass" is reasonable, but the surface area for
improvement is small.

## What's left

The current strictness doc explicitly lists what was *not* changed in the
previous sweep, and that list is the realistic agenda for any next pass:

### Item 1 — `.unwrap()` / `.expect()` audit

Production-code count (excluding inline `#[cfg(test)]` modules): **~155**.
Sample distribution:

| Location                             | Count | Character                                                                         |
| ------------------------------------ | ----- | --------------------------------------------------------------------------------- |
| `src/processors/generators/tags.rs`  | 20    | All `.expect(errors::JSON_SERIALIZE)` on serializing in-memory values. Principled. |
| `src/analyzers/tera.rs`              | 11    | All `.expect(errors::INVALID_REGEX)` on static regex literals. Principled.         |
| `src/checksum.rs`                    | 3     | `.lock().unwrap()` on Mutex. Standard idiom; poison panic is correct.              |
| `src/executor/execution.rs`          | 9     | Mix of mutex locks and `.expect(errors::INVALID_PRODUCT_ID)` invariants.           |

I sampled a few categories and they all fall into one of three buckets:

1. **Mutex lock unwraps** — correct as-is. A poisoned mutex *is* a bug.
2. **Static regex `expect()` with named error constants** — correct.
   Bad regex literal is a programmer bug, not a user error.
3. **Invariant `expect()` with named error constants** (`INVALID_PRODUCT_ID`,
   `INVALID_PROCESSOR`, `JSON_SERIALIZE`) — correct. These document the
   invariant and panic with a clear message if it's violated.

I did *not* find a single `.unwrap()` that should obviously be `?`. The
strictness doc's claim — "an audit could tighten some to `?`, but this is
a separate pass with its own judgment calls" — is, on closer inspection,
optimistic. The previous author was disciplined; the unwraps that exist
are the ones that should exist.

**Recommendation**: skip this pass. Replacing principled `.expect()` with
`?` would propagate "should never happen" errors up the stack, where the
caller has nothing useful to do with them except panic with less context.

### Item 2 — `missing_docs`

`#![warn(missing_docs)]` would force every public item to have a doc
comment. The codebase has good doc comments on public APIs already, but
not exhaustively — turning this on would surface dozens of missing
`///` comments on `pub` items.

This is largely cosmetic for an internal tool. The book in `docs/src/`
covers user-facing concepts; doc comments matter most for libraries with
external consumers.

**Recommendation**: only worth doing if you intend to publish parts of
this as a library, or if you want `cargo doc --document-private-items`
to be the canonical internal reference. Otherwise: low value.

### Item 3 — `clippy::pedantic`, `clippy::nursery`, `clippy::cargo`

- **`clippy::pedantic`**: 3 warnings, all in `build.rs`. Trivial to fix
  (it's stylistic — `map(...).unwrap_or_else(...)` → `map_or_else`).
  Adding `#![warn(clippy::pedantic)]` after the fix is a real, small win.
- **`clippy::nursery`**: 0 warnings. Adding the lint is free; it'll just
  be ready to catch new code.
- **`clippy::cargo`**: 23 warnings, all about transitive dep duplication
  driven by upstream crates. Not actionable from this repo.

**Recommendation**: do `pedantic` and `nursery`. Skip `cargo`.

### Item 4 — narrow the `use crate::config::*;` glob in `builder/mod.rs`

The strictness doc flagged this and chose to leave it. The argument was
"~15 symbols and risks churn". I'd weigh this differently — the risk of
glob imports is silent shadowing when `config::*` gains a new export.
Replacing the glob with an explicit list is mechanical and a one-time
cost. Worth doing.

### Item 5 — additional crate-level lints worth considering

These are not in the strictness doc but are common "strict Rust" knobs:

- `#![warn(unsafe_code)]` — would flag any `unsafe` block. The
  codebase appears to have none, so this is free insurance against
  accidentally introducing unsafe code in the future.
- `#![warn(unreachable_pub)]` — flags `pub` items that aren't actually
  reachable from outside the crate. Helps catch over-exposed
  visibility.
- `#![warn(rust_2018_idioms)]` — warn on pre-2018 idioms.
- `#![warn(trivial_casts, trivial_numeric_casts)]` — flag `as` casts
  that don't actually do anything.
- `#![warn(unused_crate_dependencies)]` — flag declared deps that
  aren't actually used. Useful when a feature gets removed but a dep
  stays in `Cargo.toml`.

Each of these is a one-line addition to the lint header, with a follow-up
to fix whatever it surfaces.

## Proposed plan

If you want a concrete pass, the smallest useful one is:

1. Fix the 3 `clippy::pedantic` warnings in `build.rs` and add
   `#![warn(clippy::pedantic)]` to the crate root.
2. Add `#![warn(clippy::nursery)]` (already 0 warnings).
3. Add `#![warn(unsafe_code)]`, `#![warn(unreachable_pub)]`,
   `#![warn(unused_crate_dependencies)]` and fix what surfaces.
4. Replace the `use crate::config::*;` glob in `builder/mod.rs` with an
   explicit list.

This is a few hours of work, and meaningful — every one of these adds a
real check that would catch a real future bug.

The unwrap audit (item 1) and `missing_docs` (item 2) I'd actually
recommend *against* doing at this time. The unwraps are principled; the
missing docs are not what the user-facing book is for.

## Open questions for the user

1. Should I do the small concrete plan above (items 3-5 + glob), or do
   you want a more aggressive pass that I'd push back on?
2. Are you OK with `clippy::pedantic` adding stylistic warnings to new
   code (e.g. preferring `map_or_else` over `map().unwrap_or_else()`)?
   It's not always cleaner.
3. Anything specific you've seen in the codebase that feels lax to you?
   If so, name it — these doc-driven sweeps are blunt; a specific
   complaint is much sharper than my best-guess agenda.
