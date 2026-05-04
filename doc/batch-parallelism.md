# Feature design: multi-core batch processing

## Status

Draft — awaiting review.

## Origin

`problems.txt`: "when doing batch processing we are not using multi core.
What can we do about it?"

## The current state

Parallelism in rsconstruct happens at three layers, only two of which actually
run in parallel today:

1. **Level parallelism** (works). One topological level of the build graph is
   processed at a time. Within a level, `thread::scope` spawns one thread per
   batch-group plus extra threads chunked over the non-batch items
   (`src/executor/execution.rs:260`). Different processors run concurrently.
   This is the parallelism the user already sees.

2. **Within-batch parallelism** (does NOT work). Inside a single batch group
   (e.g. one `ruff` processor's 1000 files), the items are processed
   *serially*:

   - Items are grouped into chunks of `batch_size`.
   - Each chunk calls `processor.execute_batch(ctx, products)`.
   - The default `execute_batch` (`src/processors/mod.rs:899`) is just
     `products.iter().map(|p| self.execute(ctx, p)).collect()` — a serial
     loop, one subprocess per product.
   - The checker/generator wrappers (`execute_checker_batch`,
     `execute_generator_batch`) call the tool *once* on the whole list,
     deliberately. Inside that single subprocess, parallelism is up to the
     external tool (mostly: not parallel).

3. **External-tool parallelism** (out of scope). Whether `clang-tidy` itself
   runs N files in parallel inside one invocation is not something we
   control from rsconstruct.

So the user's observation is exact: when a single processor like `pandoc`
(supports_batch=true) is asked to rebuild 200 files, those 200 invocations
happen sequentially within the batch-group thread, even on a 16-core box.

## What "batch" means in the current model — three flavors

Reading the code carefully, "batch" is overloaded across three distinct
shapes:

**Flavor A — fan-out batch.** The default `execute_batch`. The processor
declares `supports_batch = true` but provides no special batching logic, so
we just call `execute()` per product in a loop. This is the *most common*
case (every processor that doesn't override `execute_batch`). It's also the
case where parallelism would be a clean win: each `execute()` is independent
and typically spawns its own subprocess.

**Flavor B — single-invocation batch (checkers).** `execute_checker_batch`.
The tool accepts an arbitrary number of paths in one invocation
(`shellcheck a.sh b.sh c.sh ...`). The whole point is to amortize the
subprocess startup cost. Splitting the path list across cores would partly
undo that win — you'd save wall-clock time on the cores but pay subprocess
startup N times instead of once.

**Flavor C — single-invocation batch (generators).** `execute_generator_batch`.
Same shape as B but with (input, output) pairs.

These three flavors need different parallelism strategies. Treating "batch
processing" as a single thing is the source of the current limitation.

## Proposal

### Flavor A — parallelize within the batch chunk

Replace the default `execute_batch` body with a rayon parallel iterator:

```rust
fn execute_batch(&self, ctx: &crate::build_context::BuildContext, products: &[&Product]) -> Vec<Result<()>> {
    use rayon::prelude::*;
    products.par_iter().map(|p| self.execute(ctx, p)).collect()
}
```

This is a one-line change in `src/processors/mod.rs:899` plus a `rayon`
dependency. Every processor that uses the default implementation
automatically becomes parallel within a batch chunk. Rayon's global thread
pool means we don't need our own thread management.

But: rayon's default thread pool is `num_cpus`, which compounds with our
existing per-level parallelism. If level parallelism already runs 8 batch
groups in parallel on an 8-core box, and each group then forks 8 ways, we
have 64 concurrent subprocesses fighting over the cores. The user
explicitly chose `--parallel N` to bound this; we must not silently exceed
it.

The fix is to use the *same* limit. Two options:

- **Option A1**: install a custom rayon thread pool sized to `--parallel`.
  Rayon's `ThreadPoolBuilder::new().num_threads(self.parallel).build()` plus
  `pool.install(|| products.par_iter()...)` keeps the global limit.
- **Option A2**: don't use rayon at all; reuse the existing semaphore in
  `execute()` (each `execute()` already takes a permit from the
  per-processor / global semaphore). Then a plain serial loop *appears*
  serial but each `execute()` blocks on the semaphore, allowing other
  threads (other batch groups) to take permits. This is what already
  happens — and the user reports it doesn't feel parallel, suggesting the
  semaphores aren't being released eagerly enough or the structure
  serializes elsewhere.

I'd pick **A1**. It is the more direct fix: the parallelism is visible in
the code that does the work, not buried in semaphore acquisition order.
The rayon pool is sized once at executor init.

**Estimated impact**: large for projects dominated by many independent files
in one processor (typical: linters, formatters, single-file
generators). On a 16-core box building 1000 markdown files with a
single-file generator, this should be a 10-15× wall-clock speedup,
limited by I/O and process spawn overhead.

### Flavor B — leave alone, optionally chunk

`execute_checker_batch` is *already* the optimization. Splitting the path
list defeats its purpose unless the path list is so large that one
invocation hits an `argv` length limit or the tool itself hangs.

Recommend: do nothing for now. If specific tools benefit from N-way
splitting, the processor can override `execute_batch` explicitly to split
into N rayon-parallel sub-batches. We don't make this the default.

### Flavor C — same as B

`execute_generator_batch`: leave alone.

### A new processor capability flag

`supports_batch: bool` is currently doing two jobs: "this processor wants
batch input" (flavor B/C) and "this processor doesn't care, the default
applies" (flavor A). The parallelism story is different for each, so the
flag should reflect that.

Proposal: add a new field `batch_kind: BatchKind`:

```rust
pub enum BatchKind {
    /// Default: many independent execute() calls. Parallel-safe.
    /// Wall-clock time scales with cores.
    FanOut,
    /// One subprocess per chunk that handles all paths internally.
    /// Parallelism is up to the tool.
    SingleInvocation,
    /// This processor does not support batching at all.
    None,
}
```

`supports_batch` becomes derived: `batch_kind != None`. The executor reads
`batch_kind` and chooses the parallelism strategy accordingly. This is a
mechanical migration; every existing processor maps to one of the three.

This is the bigger change, but it makes the intent visible in the code.
Without it, "we made `execute_batch` parallel by default" silently turns
every checker into a worse version of itself. Tagging the kind explicitly
prevents that.

### Threading + I/O ordering

A real risk with parallel `execute()` calls: the existing code emits
progress, json events, and timing in product order from a single batch-group
thread. Switching to rayon makes that order non-deterministic. We need
ordered output for log readability — emit per-product status only after
`par_iter` collects, in input order. That's natural with `par_iter().map(...).collect()`
since `collect()` preserves order; the per-product side effects (println,
json events) move *outside* the par_iter into a serial post-processing loop.

This means moving the existing emission code in `execute()` into
post-processing in `execute_batch`. Doable but mildly invasive. Worth a
spike before fully committing.

## Open questions for the user

1. **Scope**: ship Flavor A only (the easy big win) and leave B/C alone, or
   do the `BatchKind` refactor too? Flavor A alone is ~50 lines and
   probably 80% of the user-visible benefit; the refactor is ~300 lines
   and unlocks more careful per-processor tuning later.

2. **Rayon vs. native threads**: rayon adds a dependency we don't currently
   have. The codebase already uses `std::thread::scope` — the same API
   could express within-batch parallelism (chunk into N, spawn N threads,
   join). Slightly more code, no new dep. Preference?

3. **Parallelism limit**: should within-batch parallelism count against the
   same `--parallel N` budget as level parallelism, or have its own limit
   (e.g. `--batch-parallel`)? My read is the same budget; otherwise
   `--parallel` becomes meaningless.

4. **Per-processor opt-out**: even within Flavor A, some processors might
   not be safe (writes to a shared file, mutex on a global resource). Do
   we want a `parallel_safe` flag per processor for opting out, defaulting
   to true? Or trust the existing isolation (separate subprocesses,
   separate output paths)?

5. **Output ordering**: for non-deterministic parallel output, is strict
   input order required, or is "all results before next batch chunk"
   enough? The latter is much simpler.
