# Feature: Per-Processor `max_jobs`

## Problem

When running `rsconstruct build -j 20`, all processors run with the same parallelism.
Processors like `marp` spawn heavyweight subprocesses (headless Chromium via Puppeteer),
and 20 concurrent Chromium instances cause non-deterministic `TargetCloseError` crashes
due to resource exhaustion.

## Desired Behavior

Allow each processor to declare a `max_jobs` limit in `rsconstruct.toml`:

```toml
[processor.marp]
formats = ["pdf"]
max_jobs = 4
```

With `-j 20`, marp would run at most 4 concurrent jobs while other processors use the full 20.

`max_jobs` unset or `0` means "use the global `-j` value" (current behavior).

## Implementation Plan

### 1. Add `max_jobs` field to processor configs

**File:** `src/config/processor_configs.rs`

Add to the `generator_config!` macro (all variants) and checker config structs:

```rust
#[serde(default)]
pub max_jobs: Option<usize>,
```

Add to `Default` impl (`max_jobs: None`) and `KnownFields` list.

### 2. Expose `max_jobs()` on the `ProductDiscovery` trait

**File:** `src/processors/mod.rs`

```rust
fn max_jobs(&self) -> Option<usize> { None }
```

Each processor implementation returns `self.config.max_jobs`.

### 3. Build a per-processor semaphore map in the executor

**File:** `src/executor/mod.rs`

Add to `ExecutorOptions` or build during executor construction:

```rust
pub processor_max_jobs: HashMap<String, usize>,
```

Constructed from the processor map by calling `max_jobs()` on each processor.

### 4. Use semaphores in the dispatch loop

**File:** `src/executor/execution.rs` (lines 177-203)

Create an `Arc<Semaphore>` per processor that has a `max_jobs` limit.
In the execution loop:

- **Batch groups:** If the processor has `max_jobs`, the batch thread acquires a permit
  before executing each chunk, limiting concurrent Chromium (or similar) processes.
- **Non-batch items:** Instead of dividing all non-batch items into `parallel` chunks
  regardless of processor, group by processor first. Items from limited processors
  get their own chunking (min of `max_jobs` and `parallel`), others use global `parallel`.

### 5. Config display

Ensure `rsconstruct processors config marp` and `rsconstruct config show` display
the `max_jobs` field.

## Files to Modify

1. `src/config/processor_configs.rs` - add `max_jobs` field to macros and manual configs
2. `src/processors/mod.rs` - add `max_jobs()` to `ProductDiscovery` trait
3. `src/processors/*.rs` - implement `max_jobs()` for each processor
4. `src/executor/mod.rs` - add semaphore map to `ExecutorOptions`
5. `src/executor/execution.rs` - semaphore-based dispatch in the level loop
6. `src/builder/build.rs` - build the processor limits map and pass to executor

## Alternatives Considered

- **`batch_size` workaround:** Setting `batch_size` limits items per batch invocation,
  but batch mode runs sequentially within one process, making it slow.
- **Global lower `-j`:** Works but penalizes lightweight processors unnecessarily.
