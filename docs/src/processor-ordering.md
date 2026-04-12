# Processor Ordering

When two processors touch the same files or cooperate on a shared workspace, the question of "which runs first?" inevitably comes up. This chapter explains how rsconstruct answers that question today, how other build systems approach it, the dilemmas that show up in practice, and why rsconstruct has deliberately avoided adding explicit ordering knobs so far.

## How rsconstruct orders today

**rsconstruct has no explicit cross-processor ordering configuration.** Ordering is derived entirely from the data-flow graph:

- Each product (a unit of work from a processor) declares `inputs` and `outputs`.
- If product A's `inputs` contains a path that product B's `outputs` also contains, A depends on B — B runs first.
- Products with no such relationship are considered independent and may run in parallel (within the same topological level).

That's the whole mechanism. The `BuildGraph` performs a topological sort on this implicit graph and the executor processes levels in order. See [Cross-Processor Dependencies](cross-processor-dependencies.md) for the data-flow story.

There is **no** `depends_on`, `mustRunAfter`, `before`, `after`, `priority`, or `stage` field anywhere in `rsconstruct.toml`. If two processors write into the same directory without any file dep between them, their order is undefined and may vary between runs.

## How other tools handle it

### Bazel, Buck2

**No explicit ordering.** Rules declare `srcs`, `deps`, and `outs`. The scheduler orders actions strictly by the DAG of declared inputs/outputs. Hermeticity is a first-class value — if you need something to run before something else, you model it as a data dependency. If a rule B needs rule A's side effect but not its output, you fabricate a marker file: A outputs `a.done`, B takes `a.done` as an input.

Bazel's design intent: if you need ordering without data flow, you're modeling the problem wrong. The graph should tell the truth about what depends on what.

### Make, Ninja

Data-flow ordering via rules (`foo.o: bar.h`). Ninja adds **order-only dependencies** — the `||` separator in `build.ninja`. An order-only dep means "run A before B" without "rebuild B when A changes". This is useful for things like "create `out/` before any rule tries to write into it". It's the minimum viable ordering primitive: pure ordering, no rebuild semantics.

### Gradle

**Has explicit ordering primitives**, three of them:

- `dependsOn` — real dependency: running B automatically runs A first (even if A would otherwise be skipped).
- `mustRunAfter` — ordering constraint: if both A and B are in the scheduled set, A runs first; but running B does NOT pull A in.
- `shouldRunAfter` — soft ordering hint: honored when possible, may be violated to enable parallelism.

Gradle's ecosystem (Android, JVM tooling, packaging/signing pipelines) has more real-world "unrelated tasks that still need ordering" cases — e.g., signing must happen after packaging even though they don't share a file output. The three-level hierarchy lets users pick the right strength.

### CMake

`add_dependencies(targetA targetB)` enforces ordering at the target level, beyond file-level rules. Used mostly for custom targets that don't produce tracked output files — the bridge when file-based ordering isn't sufficient.

### Cargo, SBT

No explicit cross-crate ordering. Everything flows from `[dependencies]` / library deps → data flow → topological sort. Same posture as Bazel.

### Summary table

| Tool         | Explicit ordering knobs               | Philosophy                                       |
|--------------|---------------------------------------|--------------------------------------------------|
| Make / Ninja | Order-only deps (`\|\|`)              | Bridge when file deps aren't enough              |
| Bazel, Buck2 | **None**                              | Hermeticity; all ordering comes from data flow   |
| Cargo, SBT   | **None**                              | Same as Bazel                                    |
| Gradle       | `dependsOn`, `mustRunAfter`, `shouldRunAfter` | Real-world tasks have non-data ordering needs |
| CMake        | `add_dependencies`                    | Bridge for "phantom" custom targets              |
| rsconstruct  | **None** (currently)                  | Same as Bazel                                    |

## The dilemmas

Adding explicit ordering feels useful but carries real risks. Here are the tradeoffs.

### Dilemma 1: does ordering imply rebuild?

Say `[processor.b] after = ["a"]`. If A's output changes, should B rebuild?

- If **yes**, `after` is just `dependsOn` — which we already have through data flow. It's redundant.
- If **no**, `after` is pure ordering (`mustRunAfter`). But then it silently lies about the true dependency graph: a user might add `after = ["a"]` because they "know" B consumes A's side effect, but rsconstruct won't invalidate B's cache when A changes. Stale caches follow.

Gradle copes because it has three flavors. Adding one flavor is usually wrong; adding three is complexity creep.

### Dilemma 2: declared vs. inferred

rsconstruct already infers ordering from `inputs`/`outputs`. Adding another channel means:

- Two sources of truth for the dependency graph.
- Debugging "why did B run after A?" now requires checking both the data flow AND the explicit config.
- Mistakes compound: a user adds `after = ["a"]` but forgets that they ALSO removed the data dep; now B runs after A but doesn't actually consume anything from it.

### Dilemma 3: encourages side-effects

If ordering knobs exist, they become the path of least resistance for modeling side effects:

> "My script also writes to `/tmp/cache_seed.json`, just declare `after` and it'll work."

Side-effectful processors are an anti-pattern in any incremental build system — the cache can't know when they changed, when to rerun them, or what invalidates them. Every ordering primitive that doesn't touch the cache makes side effects easier to introduce.

### Dilemma 4: the "fix-up pass" case

The one case where data flow struggles: a processor that runs *after* everything else has written to a shared directory and modifies the result. Examples:

- **Minification**: take everything in `dist/` and minify it after all generators have produced their outputs.
- **Post-processing**: add cache-busting hashes to filenames, rewrite links, compress.

In Bazel, you model this as a rule with `srcs = glob(["dist/**"])`. But with lazy generators (outputs that didn't exist when the scan ran), globs can miss things.

Reasonable fixes without adding ordering knobs:

1. Have the fix-up processor declare its inputs explicitly as the output files of the generators. Works but requires enumeration.
2. Re-scan globs after each dependency level so the fix-up step sees newly-generated files. Correct, but costlier.
3. Make the fix-up a Creator with the whole `dist/` as its `output_dir`. Our shared-output-directory logic handles this cleanly ([see that chapter](shared-output-directory.md)), but now the fix-up operates in-place on files owned by others, which touches the "files owned by other products" rule.

None of these is wonderful, but none requires a new ordering primitive.

### Dilemma 5: parallelism is already constrained

If ordering becomes a first-class concept, users will sprinkle `after = [...]` for safety and the scheduler will serialize work that could have run in parallel. Bazel's aggressive parallelism comes partly from refusing to accept unprincipled ordering constraints.

## Why rsconstruct hasn't added ordering

The posture we've picked (for now):

1. **Data flow is the truth.** Every time ordering matters, there is a real data dependency. Expose it as an input/output rather than as a separate ordering rule.
2. **Shared output directories are handled without ordering.** The [Shared Output Directory](shared-output-directory.md) design lets multiple processors contribute to one folder in any order; the cache stays correct per-processor.
3. **The cost of adding explicit ordering is high**: it creates a second channel for dependencies, invites side-effect-oriented thinking, and rarely solves a problem that couldn't be solved by modeling the data flow properly.

## When we would add explicit ordering

If a real use case appears where:

- Data flow genuinely cannot express the dependency (no file is consumed, only a side effect).
- The alternative (adding a marker file or input_glob re-scan) is significantly worse than adding a knob.
- The feature can be specified with clear rebuild semantics (pick one of: forces rerun / does not force rerun; do not leave it ambiguous).

Then the most likely shape is a single **`after = ["processor_name"]`** field with Gradle's `mustRunAfter` semantics:

- Affects ordering only when both processors are already scheduled.
- Does NOT add a rebuild trigger.
- Does NOT force the referenced processor to run.

This is the smallest, most honest knob. It doesn't pretend to be a data dependency; it doesn't change cache invalidation; it only constrains scheduling.

Until that case is concrete, the answer is: model ordering through data flow. The graph should tell the truth.

## Alternative: Output Prediction

Another way to close the gap without adding ordering knobs: make opaque Creators (mkdocs, Sphinx, Jekyll) **transparent** by discovering their outputs in advance.

Instead of the Creator declaring `output_dirs = ["_site"]` (opaque — "something goes in here"), it would declare (or generate) the exact file list it will produce:

```toml
[processor.creator.mkdocs]
command         = "mkdocs build --site-dir _site"
predict_command = "./list-mkdocs-outputs.sh"   # prints one output path per line
output_dirs     = ["_site"]
```

rsconstruct would run `predict_command` at graph-build time, turn each printed path into a declared `outputs` entry, and promote the Creator to a per-file Mass Generator. After that, the entire "how do we order two processors that both write into `_site/`?" question dissolves — every file has exactly one declared owner, and the normal Generator/data-flow rules apply.

**Why this is an alternative to ordering knobs:**

- **Explicit ordering** says *"we can't model this; let the user pin the order manually."*
- **Output prediction** says *"we can model this if we know the outputs; let's discover them."*

Prediction is the more principled answer — the graph ends up telling the truth about what depends on what — but it is far more expensive to do well (predictor drift, plugin ecosystems, partial-build support, validation). Ordering knobs are cheap but lie about the dependency graph.

The full tradeoff is explored in the [Output Prediction](output-prediction.md) chapter. Short version: neither is obviously better; they solve different problems and could coexist.

## See also

- [Cross-Processor Dependencies](cross-processor-dependencies.md) — how data-flow dependencies work between processors
- [Shared Output Directory](shared-output-directory.md) — how multiple processors can cooperate on one directory without ordering
- [Output Prediction](output-prediction.md) — a different approach that makes opaque Creators transparent
- [Design Notes](design.md) — broader design principles
