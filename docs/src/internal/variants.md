# Feature design: variants as a cross-processor feature

## Status

Draft — awaiting review. Two questions to answer plus a concrete proposal.

## Origin

`problems.txt`:

> I want to add a variant feature to every processor.
>   - what is the difference between two variants and the same processor twice in the same file?
>   - the cargo processor seems to have variants, what does that mean?

## Question 1 — what is cargo's variant?

The cargo processor (`src/processors/creators/cargo.rs`) accepts a
`profiles: Vec<String>` config field, defaulting to `["dev", "release"]`.
For each `Cargo.toml` source, it creates **one product per profile** in
the build graph and runs `cargo build --profile <name>` once per
product.

Concretely:

```toml
[processor.cargo]
profiles = ["dev", "release"]
```

Given `myapp/Cargo.toml`, this creates two graph products:

- `myapp/Cargo.toml → myapp/target/debug/myapp` with `variant = "dev"`
- `myapp/Cargo.toml → myapp/target/release/myapp` with `variant = "release"`

The mechanism is:

- `Product` has a `variant: Option<String>` field
  (`src/graph.rs:26`).
- `Graph::add_product_with_variant` is the constructor that sets it.
- The product's variant is part of its identity (cache key, output
  paths, descriptor key) so `dev` and `release` builds don't clash.
- At execute time, the processor reads `product.variant` to know which
  profile to pass to `cargo build`.

The cc_single_file processor has a similar concept named **compiler
profile** (`compilers = [{ name = "gcc", ... }, { name = "clang", ... }]`).
This is conceptually identical to cargo's `profiles` but uses a richer
config (per-profile flags) and is also already wired through
`Product.variant`.

So **the variant infrastructure exists in `Product`** (the field, the
cache-key contribution, the constructor) **but only two processors use
it**: cargo and cc_single_file. It is processor-driven — each processor
that wants variants reads its own config and creates one product per
variant.

## Question 2 — variants vs "same processor twice in the same file"

Concrete comparison:

### Variants (current design)

```toml
[processor.cargo]
profiles = ["dev", "release"]
```

- One processor instance, named `cargo`.
- One `[processor.cargo]` config section.
- The processor itself loops over `profiles` and creates N products per
  source.
- All products share the same processor config (apart from the variant
  string).
- Output paths are differentiated by variant (e.g., `target/debug/`
  vs `target/release/`).

### Multi-instance ("same processor twice")

```toml
[processor.cargo.dev]
cargo = "/usr/bin/cargo"
profiles = ["dev"]

[processor.cargo.release]
cargo = "/usr/bin/cargo-nightly"
profiles = ["release"]
```

- **Two processor instances**, named `cargo.dev` and `cargo.release`.
- Two independent config sections — they can have *completely different
  config*, including different `cargo` binary, different scan dirs,
  different excludes, different deps.
- Each instance scans independently and creates its own products.
- A product's processor name is the instance name (`cargo.dev`),
  not the type name (`cargo`).
- Cache keys include the instance name, so the two instances' caches
  are separate.

### Practical differences

| Aspect                               | Variants                  | Multi-instance              |
| ------------------------------------ | ------------------------- | --------------------------- |
| One config section or N?             | One                       | N                           |
| Can sub-configs differ?              | No (only the variant name)| Yes (every field can differ)|
| Discovers files independently?       | No (one scan, N products) | Yes (N independent scans)   |
| Naming in error messages             | `[cargo]`                 | `[cargo.dev]`               |
| Best for                             | Same build, different output target (debug+release of same code) | Different scopes (kernel vs userspace), different tools, different scan rules |

The two are not redundant. They solve different problems.

**Variants** answer: "build the same source N times with different
build settings". Cheap to declare (one extra string per profile),
shares the source-discovery scan, the products are co-located in the
graph.

**Multi-instance** answers: "I have two distinct sets of files that
both need cargo, with different rules for each set". Each instance is
fully independent — different src_dirs, different deps_inputs,
different excludes. The cost is duplicated config.

There are workloads where you'd combine both:

```toml
[processor.cargo.kernel]
src_dirs = ["kernel-app"]
profiles = ["dev", "release"]

[processor.cargo.userspace]
src_dirs = ["app"]
profiles = ["release"]
```

That's two instances, each producing its own variants. The graph would
have four products from this declaration (kernel/dev, kernel/release,
userspace/release).

## The user's request — "add a variant feature to every processor"

Reading the question literally: every processor (markdownlint, ruff,
mypy, mermaid, …) should be able to take a list of variants and run
N times.

Reading it generously: every processor should be able to participate
in the variant mechanism so a *project* can declare "this is a release
build" or "this is a debug build" globally and processors that care
will pick that up.

The two readings have very different implications.

### Reading A: per-processor variants

Add `variants: Vec<String>` (or similar) to `StandardConfig` so every
processor can opt into running N times. Each processor decides what to
do with the variant at execute time. Most processors would ignore it.

What it would look like for a checker like ruff:

```toml
[processor.ruff]
variants = ["py3.10", "py3.13"]
```

Should ruff run twice on the same file, once per variant? What changes
between the runs? If ruff itself doesn't read `product.variant`, the
two runs are byte-identical and the second is wasted work. If ruff
does read it… we need to define what each processor's "variant
contract" is.

For most checkers/linters, this question has no good answer. ruff lints
Python code. It doesn't have a "py3.10 mode" that the user would want
to alternate with a "py3.13 mode" — the version is a config flag
(`--target-version`), not a build dimension. The right way to do that
*today* is multi-instance:

```toml
[processor.ruff.py310]
args = ["--target-version=py310"]

[processor.ruff.py313]
args = ["--target-version=py313"]
```

This already works.

So per-processor variants without a per-processor variant contract
doesn't add capability. It would be a hammer in search of a nail.

### Reading B: project-level "build profile" (kernel-style)

A single global `profile` (or `target`, or `mode`) flag that processors
read. e.g.:

```bash
rsconstruct build --profile=release
```

This profile is available to processors via `BuildContext`. Each
processor decides whether and how to use it:

- cargo: passes it as `--profile`
- cc_single_file: looks up the matching `compilers[].name`
- markdownlint: ignores it
- ruff: ignores it (or reads a config field that maps profiles to args)

The user's third bullet ("the cargo processor seems to have variants,
what does that mean?") suggests they noticed the existing per-processor
variant concept and want it generalized. Reading B is the
generalization.

This is actually a much smaller change than reading A:

- Add `--profile=NAME` to the build CLI (and `clean`, `watch`).
- Pipe it into `BuildContext`.
- Processors that want it call `ctx.profile()`.
- Default profile: `"default"` (or empty).

The cargo processor's `profiles: Vec<String>` could stay as-is for
back-compat, with a new optional `profile_for_global_default: ...`
mapping that specifies which cargo profile to use when the global
profile is "default" / "release" / etc.

Most processors do nothing differently; the few that care (cargo,
cc_single_file, future others) opt in.

### Reading C: "kernel config system" approach

The user has a separate problem in `problems.txt` about wanting a
config system like the kernel's. That's different from variants but
adjacent: kernel `make menuconfig` produces a single `.config` that
selects features at build time, and the build adapts globally. If
we go that direction, "variants" might be a special case of "config
profiles" — a config profile being a saved set of toggles, and one
of the toggles is `MODE=release`.

That's a much bigger feature and is its own line in problems.txt. I'd
treat variants and the kernel-config system as separate items: ship
variants now, design the kernel-config system separately.

## Recommendation

Go with **reading B**: add a global `--profile=NAME` flag, pipe it
through `BuildContext`, and let interested processors read it.
Specifically:

1. Add `pub fn profile(&self) -> Option<&str>` to `BuildContext`.
2. Add `--profile=NAME` to `cli::Commands::Build`, `Watch`, `Clean`,
   `Fix`. Default `None`.
3. Wire the CLI value into `BuildContext::set_profile`.
4. Document the convention: processors that want to vary behavior on
   the profile read `ctx.profile()` and act accordingly. Processors
   that don't care leave it alone.
5. Update cargo to read it: if the global profile matches a cargo
   profile name, use that one. Otherwise fall back to the existing
   `profiles` list (which means "build all of these regardless of
   global").
6. Update cc_single_file the same way.
7. Add a config-key convention: any processor that wants per-profile
   *config* (not just behavior) accepts a `[[profile]]` table:

   ```toml
   [processor.somecheck]
   args = ["--strict"]

   [[processor.somecheck.profile]]
   name = "release"
   args = ["--strict", "--treat-warnings-as-errors"]
   ```

   When the global profile matches a `[[profile.NAME]]`, that section's
   fields shadow the top-level fields for this build.

(7) is the biggest piece — it requires defining how config merging
works across profiles. Could be deferred. Without it, processors can
still inspect the profile and build their own rules.

Do **not** do reading A (per-processor `variants: [...]` on every
processor) — it adds API surface without adding capability that
multi-instance doesn't already provide.

## Open questions

1. **Profile name namespace**: should `--profile` be free-form or
   should we constrain it to a known set declared in
   `[build] profiles = ["debug", "release"]`? Constrained gives better
   error messages on typos; free-form is simpler.

2. **Should the profile contribute to the cache key?** Yes, almost
   certainly — a release build and a debug build should not share
   cache entries even if their inputs are identical, because their
   outputs differ. This is what `Product.variant` already does for
   per-product variants; we'd extend the cache-key inputs to include
   the global profile too.

3. **Does the user actually want reading A or reading B?** This is the
   biggest open question and I want your read before I write code.
   "Variant feature to every processor" is ambiguous — reading B
   matches the cargo example better (cargo's profile *is* a global
   build dimension), but reading A is what a literal reading would
   suggest.

4. **Naming**: "variant", "profile", "mode", "target" — pick one for
   consistency. Cargo says "profile". Buck/Bazel say "configuration".
   I'd go with **profile** to match cargo and avoid overloading
   "variant" (which the codebase already uses for per-product
   variant strings).

5. **Migration of existing per-processor `profiles` config**: does
   `[processor.cargo] profiles = [...]` stay as-is, or does it merge
   into a global mechanism? My proposal keeps it as-is for cargo,
   adding the global profile as an additional dimension. Could
   converge later.
