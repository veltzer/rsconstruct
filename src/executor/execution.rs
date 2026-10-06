use anyhow::{Context, Result};
use indicatif::ProgressBar;
use parking_lot::{Condvar, Mutex, RwLock};
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Instant;

use crate::color;
use crate::errors;
use crate::graph::{BuildGraph, Product};
use crate::json_output;
use crate::object_store::ObjectStore;
use crate::progress;
use crate::stats::{BuildStats, ProductTiming};

use super::{
    Classification, Executor, GraphRefresh, HandlerContext, LevelWork, PreCheckResult,
    ProductAction, ReadyTracker, RestoreOutcome, SharedState, WorkItem,
};

/// Compute the effective `max_jobs` for a processor instance. The config
/// field is capped by the plugin's static `max_jobs_cap`; `None` on either
/// side means no limit on that side.
fn effective_max_jobs(name: &str, proc: &dyn crate::processor::Processor) -> Option<usize> {
    let plugin = crate::registries::processor::find_plugin(name);
    let user_set = proc.scan_config().max_jobs;
    let static_cap = plugin.and_then(|p| p.max_jobs_cap);
    match (user_set, static_cap) {
        (Some(c), Some(m)) => Some(c.min(m)),
        (Some(c), None) => Some(c),
        (None, Some(m)) => Some(m),
        (None, None) => None,
    }
}

/// Compute the effective `supports_batch` for a processor instance: the
/// plugin's static capability flag must be true, AND the user config must
/// request batching.
fn effective_supports_batch(name: &str, proc: &dyn crate::processor::Processor) -> bool {
    let plugin_ok =
        crate::registries::processor::find_plugin(name).is_some_and(|p| p.supports_batch);
    plugin_ok && proc.scan_config().batch
}

/// Chunk size for one batch group. `0` means no limit — the whole group in
/// one chunk. Batching disabled (`batch_size: None`) never forms batch
/// groups (see [`should_batch`]), so this only sees explicit sizes. Never
/// returns 0 — `chunks(0)` panics.
fn batch_chunk_size(batch_size: usize, n_items: usize) -> usize {
    if batch_size == 0 {
        n_items.max(1)
    } else {
        batch_size
    }
}

/// Whether a processor's items take the batch path. Batching requires an
/// explicit batch size (`batch_size: None` disables it entirely), a
/// processor that supports it, and more than one item actually rebuilding.
const fn should_batch(batching_enabled: bool, supports_batch: bool, rebuild_count: usize) -> bool {
    batching_enabled && supports_batch && rebuild_count > 1
}

/// One piece of work for a worker: a single product, or a chunk of one
/// batching processor's products run in one tool invocation.
enum Unit {
    Single(WorkItem),
    Batch {
        processor: String,
        items: Vec<WorkItem>,
    },
}

impl Unit {
    fn processor(&self) -> &str {
        match self {
            Self::Single(item) => &item.product.processor,
            Self::Batch { processor, .. } => processor,
        }
    }

    fn product_ids(&self) -> Vec<usize> {
        match self {
            Self::Single(item) => vec![item.product_id],
            Self::Batch { items, .. } => items.iter().map(|i| i.product_id).collect(),
        }
    }
}

/// The units waiting for a worker, and how many each processor has running.
struct QueueState {
    units: VecDeque<Unit>,
    running: HashMap<String, usize>,
    closed: bool,
}

/// Work shared by the worker pool. A worker takes the first unit whose
/// processor is below its `max_jobs` cap, so a capped processor never holds
/// a worker idle — the semaphores this replaces made a worker that picked
/// such a product block until a permit freed, while other work waited.
struct WorkQueue {
    state: Mutex<QueueState>,
    changed: Condvar,
}

impl WorkQueue {
    fn new() -> Self {
        Self {
            state: Mutex::new(QueueState {
                units: VecDeque::new(),
                running: HashMap::new(),
                closed: false,
            }),
            changed: Condvar::new(),
        }
    }

    fn push(&self, units: Vec<Unit>) {
        if units.is_empty() {
            return;
        }
        self.state.lock().units.extend(units);
        self.changed.notify_all();
    }

    /// Block until a unit can run under `caps`, and return it; `None` once
    /// the queue is closed.
    fn pop(&self, caps: &HashMap<String, usize>) -> Option<Unit> {
        let mut state = self.state.lock();
        loop {
            if state.closed {
                return None;
            }
            let runnable = state.units.iter().position(|unit| {
                caps.get(unit.processor()).is_none_or(|&cap| {
                    state.running.get(unit.processor()).copied().unwrap_or(0) < cap
                })
            });
            if let Some(index) = runnable {
                let unit = state.units.remove(index).expect("index from position");
                *state
                    .running
                    .entry(unit.processor().to_string())
                    .or_insert(0) += 1;
                return Some(unit);
            }
            self.changed.wait(&mut state);
        }
    }

    /// A unit of `processor` finished: free its slot under the cap.
    fn done(&self, processor: &str) {
        let mut state = self.state.lock();
        if let Some(n) = state.running.get_mut(processor) {
            *n -= 1;
        }
        drop(state);
        self.changed.notify_all();
    }

    /// Wake every worker and make `pop` return `None`; units still queued
    /// are dropped (the run is over or was stopped).
    fn close(&self) {
        self.state.lock().closed = true;
        self.changed.notify_all();
    }
}

/// Reports a unit's products as finished to the coordinator when dropped —
/// including when the unit panicked, so the coordinator is never left
/// waiting for products that will not report.
struct FinishOnDrop<'q> {
    ids: Vec<usize>,
    processor: String,
    queue: &'q WorkQueue,
    tx: mpsc::Sender<Vec<usize>>,
}

impl Drop for FinishOnDrop<'_> {
    fn drop(&mut self) {
        self.queue.done(&self.processor);
        let _ = self.tx.send(std::mem::take(&mut self.ids));
    }
}

/// Remove existing output files before executing a product.
///
/// Cache objects are stored read-only to prevent corruption via hardlinks.
/// When a restored hardlink exists and a rebuild is needed, the tool cannot
/// overwrite the read-only file. Removing outputs first ensures the tool
/// can create fresh files.
///
/// For products with `output_dirs` (Creators), we do NOT wipe entire directories
/// because other processors may contribute files to the same directory. Instead,
/// we remove just the files recorded in this product's last tree descriptor
/// (falling back to creating empty dirs if there is no prior tree).
///
/// "Last" means the tree that is actually on disk, which is not the tree
/// under the current descriptor key: that key mixes in the new input
/// checksum, and on a real rebuild no descriptor exists for it yet. The
/// object store keeps a pointer from the product's `owner_key` to the
/// descriptor it last built or restored (`record_last_tree`), and that is
/// what names the files to unlink. Without it every rebuild after a
/// hardlink restore failed: sphinx and friends open their outputs for
/// writing in place, and a read-only cache hardlink refuses.
pub(super) fn remove_stale_outputs(
    product: &Product,
    object_store: &ObjectStore,
    input_checksum: &str,
) -> anyhow::Result<()> {
    if !product.output_dirs.is_empty() {
        // Creator-style: remove only files we owned in the previous run.
        // Files that belong to other processors (sharing the same directory)
        // are left alone and restored/rebuilt independently.
        let cache_key = product.descriptor_key(input_checksum);
        let mut stale: BTreeSet<PathBuf> = object_store
            .previous_tree_paths(&cache_key)
            .into_iter()
            .collect();
        match object_store.last_tree_key(&product.owner_key()) {
            Some(last_key) => stale.extend(object_store.previous_tree_paths(&last_key)),
            // No pointer: the outputs on disk predate the pointer table
            // (built by an older rsconstruct). Fall back to the one signature
            // a restore leaves behind that a tool cannot write over.
            None => stale.extend(restored_hardlinks_under(&product.output_dirs)),
        }
        for file in stale {
            if file.exists() {
                fs::remove_file(&file).with_context(|| {
                    format!("Failed to remove stale output: {}", file.display())
                })?;
            }
        }
        // Ensure output dirs still exist (the tool may assume they do)
        for output_dir in &product.output_dirs {
            fs::create_dir_all(output_dir.as_ref()).with_context(|| {
                format!(
                    "Failed to create output directory: {}",
                    output_dir.display()
                )
            })?;
        }
    }
    for output in &product.outputs {
        if output.exists() {
            fs::remove_file(output)
                .with_context(|| format!("Failed to remove stale output: {}", output.display()))?;
        }
    }
    Ok(())
}

/// Files under `dirs` that a hardlink restore put there: read-only and
/// sharing their inode with a cache object, so with a link count above one.
/// Nothing else in a build tree has both properties, and these are exactly
/// the files a rebuilding tool cannot open for writing.
fn restored_hardlinks_under(dirs: &[Arc<PathBuf>]) -> Vec<PathBuf> {
    use std::os::unix::fs::MetadataExt;
    dirs.iter()
        .flat_map(|dir| crate::object_store::walk_files(dir))
        .filter(|file| {
            fs::metadata(file).is_ok_and(|m| m.permissions().readonly() && m.nlink() > 1)
        })
        .collect()
}

/// Per-processor progress counters shared across non-batch threads.
struct ProgressCounters {
    total_per_processor: Arc<HashMap<String, usize>>,
    current_per_processor: Arc<Mutex<HashMap<String, usize>>>,
}

/// Context shared by every worker for the whole run.
struct LevelContext<'b> {
    /// Read briefly by workers (who owns a path, when caching a creator's
    /// tree); written by the coordinator when re-analysis adds inputs. A
    /// worker runs its tool from its own copy of the product, never holding
    /// this lock.
    graph: &'b RwLock<BuildGraph>,
    object_store: &'b ObjectStore,
    keep_going: bool,
    timings: bool,
    shared: &'b SharedState,
    pb: &'b ProgressBar,
    build_start: Instant,
    /// The up-front prediction per product, used only to keep the progress
    /// bar (sized from it) honest when dispatch decides differently.
    predicted: &'b HashMap<usize, ProductAction>,
}

impl Executor<'_> {
    /// Execute all products in the graph that need rebuilding.
    ///
    /// `classification` is the up-front prediction from
    /// [`classify_products`](super::classify_products); it only sizes the
    /// progress bar. Each product's fate is decided when it is dispatched,
    /// after all its dependencies ran: its input checksum is taken then,
    /// and the policy classifies it against that. A dependency that rebuilt
    /// to identical bytes therefore leaves the product skippable, and the
    /// checksum the outputs are cached under is the one the product read.
    ///
    /// `refresh`, when given, runs as products complete (see
    /// [`GraphRefresh`]); the graph is mutable for its sake.
    pub fn execute(
        &self,
        graph: &mut BuildGraph,
        object_store: &ObjectStore,
        classification: &Classification,
        refresh: Option<&mut GraphRefresh<'_>>,
    ) -> Result<BuildStats> {
        let build_start = Instant::now();
        let order = graph.topological_sort()?;

        // Processors that verify a shared output directory after their tool
        // ran (mass generators) ask the context who owns a path; `execute`
        // never sees the graph itself.
        self.build_ctx.set_declared_outputs(
            graph
                .products()
                .iter()
                .flat_map(|p| p.outputs.iter().cloned())
                .collect(),
        );

        // Emit JSON build start event
        json_output::emit_build_start(order.len());

        let result = self.execute_parallel(graph, &order, object_store, classification, refresh);

        match result {
            Ok(mut stats) => {
                stats.total_duration = build_start.elapsed();

                // Emit JSON build summary
                json_output::emit_build_summary(
                    stats.total_processed()
                        + stats.total_skipped()
                        + stats.total_restored()
                        + stats.failed_count,
                    stats.total_processed(),
                    stats.failed_count,
                    stats.total_skipped(),
                    stats.total_restored(),
                    stats.total_duration,
                    &stats.failed_messages,
                );

                Ok(stats)
            }
            Err(e) => {
                // Emit JSON build summary even on failure
                let duration = build_start.elapsed();
                json_output::emit_build_summary(
                    order.len(),
                    0,
                    1,
                    0,
                    0,
                    duration,
                    &[e.to_string()],
                );
                Err(e)
            }
        }
    }

    /// Execute products in parallel as their dependencies allow.
    ///
    /// A coordinator (this thread) tracks which products are ready —
    /// every dependency finished — and a pool of exactly `parallel` workers
    /// runs them. Whenever products become ready, the coordinator prepares
    /// them together (checksum and policy decision at dispatch, failed
    /// dependencies, batch grouping) and queues them as units: one product,
    /// or a chunk of a batching processor's products run in one invocation.
    /// A finished unit makes its dependents ready at once; nothing waits
    /// for unrelated products, as a level barrier used to make it.
    ///
    /// `-j` is the number of workers, so it caps everything, batch units
    /// included; a processor's `max_jobs` caps how many of its units run at
    /// once.
    fn execute_parallel(
        &self,
        graph: &mut BuildGraph,
        order: &[usize],
        object_store: &ObjectStore,
        classification: &Classification,
        refresh: Option<&mut GraphRefresh<'_>>,
    ) -> Result<BuildStats> {
        let build_start = Instant::now();
        let keep_going = self.keep_going;
        let predicted: HashMap<usize, ProductAction> = classification
            .products
            .iter()
            .map(|c| (c.id, c.action))
            .collect();

        // Count total products per processor for progress display
        let mut total_per_processor: HashMap<String, usize> = HashMap::new();
        for &product_id in order {
            let product = graph
                .get_product(product_id)
                .expect(errors::INVALID_PRODUCT_ID);
            *total_per_processor
                .entry(product.processor.clone())
                .or_insert(0) += 1;
        }
        let counters = ProgressCounters {
            total_per_processor: Arc::new(total_per_processor),
            current_per_processor: Arc::new(Mutex::new(HashMap::new())),
        };
        let global_total = order.len();

        // Use the caller-provided classification for progress bar sizing.
        let work_count = classification.restore_count + classification.build_count;

        // Create progress bar sized to actual work (excludes instant skips)
        let pb = progress::create_bar(
            work_count as u64,
            self.verbose || json_output::is_json_mode() || crate::runtime_flags::quiet(),
        );

        let shared = SharedState {
            stats: Arc::new(Mutex::new(HashMap::new())),
            errors: Arc::new(Mutex::new(Vec::new())),
            failed_products: Arc::new(Mutex::new(HashSet::new())),
            failed_messages: Arc::new(Mutex::new(Vec::new())),
            failed_processors: Arc::new(Mutex::new(HashSet::new())),
            changed: Arc::new(Mutex::new(Vec::new())),
            attempted: Arc::new(Mutex::new(HashSet::new())),
            global_current: Arc::new(AtomicUsize::new(0)),
            global_total,
        };

        // max_jobs caps, enforced by the queue when it hands out units.
        let caps: HashMap<String, usize> = self
            .processors
            .iter()
            .filter_map(|(name, proc)| {
                effective_max_jobs(name, proc.as_ref()).map(|max| (name.clone(), max))
            })
            .collect();

        // The graph goes behind a lock for the run (see LevelContext::graph)
        // and comes back afterwards.
        let graph_lock = RwLock::new(std::mem::take(graph));
        let queue = WorkQueue::new();
        let (tx, rx) = mpsc::channel::<Vec<usize>>();
        let lctx = LevelContext {
            graph: &graph_lock,
            object_store,
            keep_going,
            timings: self.timings,
            shared: &shared,
            pb: &pb,
            build_start,
            predicted: &predicted,
        };

        let result = thread::scope(|s| -> Result<()> {
            let tx = tx;
            for _ in 0..self.parallel.max(1) {
                let tx = tx.clone();
                let (queue, caps, lctx, counters) = (&queue, &caps, &lctx, &counters);
                s.spawn(move || {
                    while let Some(unit) = queue.pop(caps) {
                        let _finished = FinishOnDrop {
                            ids: unit.product_ids(),
                            processor: unit.processor().to_string(),
                            queue,
                            tx: tx.clone(),
                        };
                        self.run_unit(unit, lctx, counters);
                    }
                });
            }
            // Only the workers hold senders now: if they all exit, the
            // coordinator's receive fails instead of waiting forever.
            drop(tx);
            let outcome = self.coordinate(&graph_lock, &queue, &rx, object_store, &shared, refresh);
            // Release the workers whatever happened, or the scope never ends.
            queue.close();
            outcome
        });
        *graph = graph_lock.into_inner();

        pb.finish_and_clear();
        if self.is_interrupted() {
            crate::output::info(&color::yellow("Interrupted, saving progress..."));
        }
        result?;
        Self::remove_outputs_never_rebuilt(graph, object_store, classification, &shared)?;
        Self::collect_build_stats(shared, keep_going, self.is_interrupted())
    }

    /// Remove the outputs of every product that was predicted to change but
    /// never ran: skipped because a dependency failed, or never reached
    /// because the build stopped on an error or Ctrl+C. Its inputs changed,
    /// so what is on disk is stale, and leaving it there would look like
    /// the build succeeded. Outputs used to be removed for every predicted
    /// change before the run started; removing them only now, for products
    /// that did not run, is what lets a product whose dependency rebuilt to
    /// identical bytes be skipped with its outputs intact.
    fn remove_outputs_never_rebuilt(
        graph: &BuildGraph,
        object_store: &ObjectStore,
        classification: &Classification,
        shared: &SharedState,
    ) -> Result<()> {
        let attempted = shared.attempted.lock();
        for c in &classification.products {
            if c.action == ProductAction::Skip || attempted.contains(&c.id) {
                continue;
            }
            let product = graph.get_product(c.id).expect(errors::INVALID_PRODUCT_ID);
            remove_stale_outputs(product, object_store, &c.input_checksum)?;
        }
        Ok(())
    }

    /// The coordinator loop: hand ready products to the workers, record
    /// what they finish, and re-analyze as products complete. Returns when
    /// nothing is running and nothing is ready — every product ran, or the
    /// build stopped (an error without `--keep-going`, or Ctrl+C) and the
    /// running products drained.
    fn coordinate(
        &self,
        graph: &RwLock<BuildGraph>,
        queue: &WorkQueue,
        finished: &mpsc::Receiver<Vec<usize>>,
        object_store: &ObjectStore,
        shared: &SharedState,
        mut refresh: Option<&mut GraphRefresh<'_>>,
    ) -> Result<()> {
        let mut tracker = ReadyTracker::new(&graph.read());
        loop {
            let stopping =
                self.is_interrupted() || (!self.keep_going && !shared.errors.lock().is_empty());
            if !stopping {
                let wave = tracker.take_ready();
                if !wave.is_empty() {
                    let work = self.prepare_level_work(&graph.read(), &wave, object_store, shared);
                    let units = self.units_of(work);
                    let queued: HashSet<usize> = units.iter().flat_map(Unit::product_ids).collect();
                    // Products prepare settled without running — a failed
                    // dependency, an unreadable input — are finished now,
                    // which may make more products ready.
                    let g = graph.read();
                    for &id in wave.iter().filter(|id| !queued.contains(id)) {
                        tracker.finish(&g, id);
                    }
                    drop(g);
                    queue.push(units);
                    continue;
                }
            }
            if tracker.in_flight == 0 {
                return Ok(());
            }

            // Wait for a unit to finish, then take every other report that
            // is already in.
            let mut done = finished
                .recv()
                .context("internal error: every worker exited with products in flight")?;
            while let Ok(more) = finished.try_recv() {
                done.extend(more);
            }
            let g = graph.read();
            for id in done {
                tracker.finish(&g, id);
            }
            drop(g);

            // Re-analysis of what the finished products regenerated, before
            // anything they feed is dispatched.
            let changed = std::mem::take(&mut *shared.changed.lock());
            if let Some(refresh) = refresh.as_deref_mut()
                && !changed.is_empty()
            {
                let mut g = graph.write();
                if refresh(&mut g, &changed).context("Failed to re-analyze regenerated sources")? {
                    // A new input may be the output of a product that has
                    // not run, which must now run first.
                    g.resolve_dependencies();
                    g.topological_sort()?;
                    tracker.recount(&g);
                }
            }
        }
    }

    /// Turn a prepared wave into units: each batch group's products that
    /// will run the tool become chunks of at most `batch_size` (run in
    /// parallel with each other, not one after another), and everything
    /// else — non-batching processors, and batching processors' products
    /// that skip or restore — becomes one unit per product.
    fn units_of(&self, work: LevelWork) -> Vec<Unit> {
        let LevelWork {
            batch_groups,
            non_batch_items,
        } = work;
        let mut units: Vec<Unit> = non_batch_items.into_iter().map(Unit::Single).collect();
        let mut groups: Vec<(String, Vec<WorkItem>)> = batch_groups.into_iter().collect();
        groups.sort_by(|a, b| a.0.cmp(&b.0));
        for (processor, items) in groups {
            let (to_run, settled): (Vec<WorkItem>, Vec<WorkItem>) = items
                .into_iter()
                .partition(|item| item.action == ProductAction::Build);
            units.extend(settled.into_iter().map(Unit::Single));
            let chunk_size = batch_chunk_size(
                self.batch_size
                    .expect("batch groups only form when batching is enabled"),
                to_run.len(),
            );
            let mut to_run = to_run.into_iter().peekable();
            while to_run.peek().is_some() {
                let chunk: Vec<WorkItem> = to_run.by_ref().take(chunk_size).collect();
                units.push(Unit::Batch {
                    processor: processor.clone(),
                    items: chunk,
                });
            }
        }
        units
    }

    /// Run one unit on a worker.
    fn run_unit(&self, unit: Unit, lctx: &LevelContext, counters: &ProgressCounters) {
        match unit {
            Unit::Single(item) => self.process_non_batch_chunk(
                std::slice::from_ref(&item),
                lctx,
                &counters.total_per_processor,
                &counters.current_per_processor,
            ),
            Unit::Batch { processor, items } => {
                self.process_batch_group(&processor, &items, lctx);
            }
        }
    }

    /// Pre-check a work item: handle explain, skip-if-unchanged, and cache restore.
    ///
    /// Returns `Handled` if the item was fully processed (skip/restore/failed),
    /// or `NeedsExecution` if the caller should proceed to execute the product.
    fn try_skip_or_restore(
        &self,
        item: &WorkItem,
        proc_name: &str,
        lctx: &LevelContext,
        emit_fail_event: bool,
    ) -> PreCheckResult {
        let product = &item.product;
        lctx.shared.attempted.lock().insert(item.product_id);

        if self.explain {
            let action = self.policy.explain(
                self.build_ctx,
                product,
                lctx.object_store,
                &item.input_checksum,
                self.force,
            );
            self.print_explain(product, &action);
        }

        let ctx = HandlerContext {
            product,
            id: item.product_id,
            input_checksum: &item.input_checksum,
            proc_name,
            keep_going: lctx.keep_going,
            shared: lctx.shared,
            pb: lctx.pb,
        };
        if item.action == ProductAction::Skip {
            // Unchanged: its outputs on disk are current and stay put. A
            // product predicted to build because a dependency changed lands
            // here when the dependency rebuilt to identical bytes —
            // unchanged-output pruning: no rerun, no restore, no file touched.
            self.handle_skip(product, lctx.shared);
            // The bar counts predicted work; such a product still has to
            // tick it off.
            if lctx.predicted.get(&item.product_id) != Some(&ProductAction::Skip) {
                lctx.pb.inc(1);
            }
            return PreCheckResult::Handled;
        }

        // About to restore or rebuild: clear the outputs the previous build
        // left (a restored hardlink is read-only, and a tool that writes in
        // place cannot overwrite it). Done here, per product, and not for
        // every product predicted to change before the run starts — that
        // turned a product whose dependency rebuilt to identical bytes into
        // a restore instead of a skip. Products that never get here are
        // swept at the end of the run (see `execute_parallel`). A processor
        // that replaces its own outputs (`Processor::replaces_own_outputs`)
        // is left alone: its tool run may already have written them.
        let replaces_own = self
            .processors
            .get(proc_name)
            .is_some_and(|p| p.replaces_own_outputs());
        if !replaces_own
            && let Err(e) = remove_stale_outputs(product, lctx.object_store, &item.input_checksum)
        {
            self.handle_error(&ctx, e, None);
            Self::inc_progress(lctx.pb, lctx.shared);
            return PreCheckResult::Handled;
        }
        match item.action {
            ProductAction::Restore => {
                match self.handle_restore(&ctx, lctx.object_store, emit_fail_event) {
                    RestoreOutcome::Restored | RestoreOutcome::Failed => PreCheckResult::Handled,
                    RestoreOutcome::NotRestorable => PreCheckResult::NeedsExecution,
                }
            }
            ProductAction::Skip | ProductAction::Build => PreCheckResult::NeedsExecution,
        }
    }

    /// Process one batch unit: a chunk of a batching processor's products,
    /// run in one invocation of its tool.
    fn process_batch_group(&self, proc_name: &str, items: &[WorkItem], lctx: &LevelContext) {
        if self.is_interrupted() {
            return;
        }

        let Some(processor) = self.processors.get(proc_name) else {
            return;
        };

        // Handle skip/restore for items that don't need rebuild
        let mut to_execute: Vec<&WorkItem> = Vec::new();
        for item in items {
            match self.try_skip_or_restore(item, proc_name, lctx, true) {
                // Already skipped or restored — nothing left to execute.
                PreCheckResult::Handled => {}
                PreCheckResult::NeedsExecution => to_execute.push(item),
            }
        }

        if to_execute.is_empty() || self.is_interrupted() {
            return;
        }

        let proc_total = items.len();
        let mut proc_current = items.len() - to_execute.len();

        let chunk_size = batch_chunk_size(
            self.batch_size
                .expect("batch groups only form when batching is enabled"),
            to_execute.len(),
        );

        // Process in chunks
        for chunk in to_execute.chunks(chunk_size) {
            if self.is_interrupted() || (!lctx.keep_going && !lctx.shared.errors.lock().is_empty())
            {
                break;
            }

            // Execute batch chunk
            let product_refs: Vec<&crate::graph::Product> =
                chunk.iter().map(|item| &item.product).collect();

            proc_current += chunk.len();
            if self.verbose {
                let display = product_refs
                    .iter()
                    .map(|p| self.product_display(p))
                    .collect::<Vec<_>>()
                    .join(", ");
                let gc = lctx.shared.global_current.load(Ordering::SeqCst);
                crate::output::info(&format!(
                    "[{}] ({}/{}) ({}/{}) {} {} files: {}",
                    proc_name,
                    gc + 1,
                    lctx.shared.global_total,
                    proc_current,
                    proc_total,
                    color::green("Processing batch:"),
                    product_refs.len(),
                    display
                ));
            } else {
                lctx.pb.set_message(format!(
                    "[{}] batch {} files",
                    proc_name,
                    product_refs.len()
                ));
            }

            // Stale outputs were removed per product in try_skip_or_restore;
            // just announce starts.
            for p in &product_refs {
                json_output::emit_product_start(&self.product_display(p), &p.processor);
            }

            let batch_start = Instant::now();

            // Retry: the first attempt covers the whole chunk; each retry
            // re-runs only the products that failed the previous attempt, so
            // `--retry` keeps its per-product meaning on the batch path
            // (it used to be silently ignored here).
            let max_attempts = 1 + self.retry;
            let mut final_results: Vec<Option<anyhow::Result<()>>> =
                (0..chunk.len()).map(|_| None).collect();
            let mut pending: Vec<usize> = (0..chunk.len()).collect();
            crate::processor::set_declared_tools(Some(processor.required_tools()));
            for attempt in 1..=max_attempts {
                let refs: Vec<&crate::graph::Product> =
                    pending.iter().map(|&i| product_refs[i]).collect();
                let results = processor.execute_batch(self.build_ctx, &refs);

                // Validate batch returned correct number of results
                assert_eq!(
                    results.len(),
                    refs.len(),
                    "execute_batch returned {} results for {} products (processor: {})",
                    results.len(),
                    refs.len(),
                    proc_name
                );

                let mut still_failing: Vec<usize> = Vec::new();
                for (&idx, result) in pending.iter().zip(results) {
                    if result.is_err() && attempt < max_attempts {
                        crate::output::info(&format!(
                            "[{}] {} {} (attempt {}/{}, retrying...)",
                            proc_name,
                            color::yellow("Retry:"),
                            self.product_display(product_refs[idx]),
                            attempt,
                            max_attempts
                        ));
                        still_failing.push(idx);
                    } else {
                        if result.is_ok() && attempt > 1 {
                            // Passed on a retry: report flaky, like the non-batch path.
                            let mut stats = lctx.shared.stats.lock();
                            stats.entry(proc_name.to_string()).or_default().flaky += 1;
                            crate::output::info(&format!(
                                "[{}] {} {} (passed on attempt {})",
                                proc_name,
                                color::yellow("FLAKY:"),
                                self.product_display(product_refs[idx]),
                                attempt
                            ));
                        }
                        final_results[idx] = Some(result);
                    }
                }
                pending = still_failing;
                if pending.is_empty() {
                    break;
                }
            }
            crate::processor::set_declared_tools(None);
            let batch_duration = batch_start.elapsed();

            // Process per-product results
            for (item, result) in chunk.iter().zip(final_results) {
                let result =
                    result.expect("the retry loop records a final result for every product");
                let product = &item.product;

                let ctx = HandlerContext {
                    product,
                    id: item.product_id,
                    input_checksum: &item.input_checksum,
                    proc_name,
                    keep_going: lctx.keep_going,
                    shared: lctx.shared,
                    pb: lctx.pb,
                };
                match result {
                    Ok(()) => {
                        // handle_success returns false if cache_outputs failed
                        self.handle_success(&ctx, lctx.object_store, lctx.graph, None);
                    }
                    Err(e) => {
                        self.handle_error(&ctx, e, None);
                    }
                }
                Self::inc_progress(lctx.pb, lctx.shared);
            }

            // Record batch timing for this chunk
            if lctx.timings {
                let timing = ProductTiming {
                    display: format!("batch ({} files)", product_refs.len()),
                    processor: proc_name.to_string(),
                    duration: batch_duration,
                    start_offset: Some(batch_start.duration_since(lctx.build_start)),
                };
                let mut stats = lctx.shared.stats.lock();
                let proc_stats = stats.entry(proc_name.to_string()).or_default();
                proc_stats.duration += batch_duration;
                proc_stats.product_timings.push(timing);
            }
        }
    }

    /// Process non-batch work items, one after another.
    fn process_non_batch_chunk(
        &self,
        chunk: &[WorkItem],
        lctx: &LevelContext,
        total_per_processor: &HashMap<String, usize>,
        current_per_processor: &Mutex<HashMap<String, usize>>,
    ) {
        for item in chunk {
            // Stop if interrupted or if there's an error (non-keep-going mode)
            if self.is_interrupted() || (!lctx.keep_going && !lctx.shared.errors.lock().is_empty())
            {
                break;
            }

            let product = &item.product;

            if matches!(
                self.try_skip_or_restore(item, &product.processor, lctx, true),
                PreCheckResult::Handled
            ) {
                continue;
            }

            if let Some(processor) = self.processors.get(&product.processor) {
                let ctx = HandlerContext {
                    product,
                    id: item.product_id,
                    input_checksum: &item.input_checksum,
                    proc_name: &product.processor,
                    keep_going: lctx.keep_going,
                    shared: lctx.shared,
                    pb: lctx.pb,
                };

                // Update progress counter
                let current = {
                    let mut current_guard = current_per_processor.lock();
                    let c = current_guard.entry(product.processor.clone()).or_insert(0);
                    *c += 1;
                    *c
                };
                let total = total_per_processor
                    .get(&product.processor)
                    .copied()
                    .expect(errors::PROCESSOR_NOT_IN_TOTALS);

                if self.verbose {
                    let variant_tag = product
                        .variant
                        .as_ref()
                        .map(|v| format!(":{v}"))
                        .unwrap_or_default();
                    let gc = lctx.shared.global_current.load(Ordering::SeqCst) + 1;
                    crate::output::info(&format!(
                        "[{}{}] ({}/{}) ({}/{}) {} {}",
                        product.processor,
                        variant_tag,
                        gc,
                        lctx.shared.global_total,
                        current,
                        total,
                        color::green("Processing:"),
                        self.product_display(product)
                    ));
                } else {
                    let variant_tag = product
                        .variant
                        .as_ref()
                        .map(|v| format!(":{v}"))
                        .unwrap_or_default();
                    lctx.pb.set_message(format!(
                        "[{}{}] {}",
                        product.processor,
                        variant_tag,
                        self.product_display(product)
                    ));
                }

                json_output::emit_product_start(&self.product_display(product), &product.processor);
                let product_start = Instant::now();
                let mut last_error = None;
                let max_attempts = 1 + self.retry;
                crate::processor::set_declared_tools(Some(processor.required_tools()));
                for attempt in 1..=max_attempts {
                    match processor.execute(self.build_ctx, product) {
                        Ok(()) => {
                            let duration = product_start.elapsed();
                            last_error = None; // Clear before handle_success to avoid double-failure if caching fails

                            if !self.handle_success(
                                &ctx,
                                lctx.object_store,
                                lctx.graph,
                                Some(duration),
                            ) {
                                // cache_outputs failed and error was handled
                                break;
                            }

                            // Mark as flaky if it passed on a retry
                            if attempt > 1 {
                                let mut stats = lctx.shared.stats.lock();
                                let proc_stats =
                                    stats.entry(product.processor.clone()).or_default();
                                proc_stats.flaky += 1;
                                crate::output::info(&format!(
                                    "[{}] {} {} (passed on attempt {})",
                                    product.processor,
                                    color::yellow("FLAKY:"),
                                    self.product_display(product),
                                    attempt
                                ));
                            }

                            // Record per-product duration (non-batch only)
                            {
                                let mut stats = lctx.shared.stats.lock();
                                let proc_stats =
                                    stats.entry(product.processor.clone()).or_default();
                                proc_stats.duration += duration;
                                if lctx.timings {
                                    proc_stats.product_timings.push(ProductTiming {
                                        display: self.product_display(product),
                                        processor: product.processor.clone(),
                                        duration,
                                        start_offset: Some(
                                            product_start.duration_since(lctx.build_start),
                                        ),
                                    });
                                }
                            }
                            break;
                        }
                        Err(e) => {
                            if attempt < max_attempts {
                                crate::output::info(&format!(
                                    "[{}] {} {} (attempt {}/{}, retrying...)",
                                    product.processor,
                                    color::yellow("Retry:"),
                                    self.product_display(product),
                                    attempt,
                                    max_attempts
                                ));
                                last_error = Some(e);
                            } else {
                                let duration = product_start.elapsed();
                                self.handle_error(&ctx, e, Some(duration));
                                last_error = None; // error already handled
                            }
                        }
                    }
                }
                // Safety: if we broke out of retry loop with unhandled error, handle it
                if let Some(e) = last_error {
                    let duration = product_start.elapsed();
                    self.handle_error(&ctx, e, Some(duration));
                }
                crate::processor::set_declared_tools(None);
                Self::inc_progress(lctx.pb, lctx.shared);
            }
        }
    }

    /// Collect final build stats from shared state after all levels complete.
    fn collect_build_stats(
        shared: SharedState,
        keep_going: bool,
        interrupted: bool,
    ) -> Result<BuildStats> {
        let final_stats = Arc::try_unwrap(shared.stats)
            .map_err(|_| anyhow::anyhow!("internal error: outstanding Arc reference to stats"))?
            .into_inner();
        let mut stats = BuildStats::default();
        for (_, proc_stats) in final_stats {
            stats.add(proc_stats);
        }

        let final_failed = Arc::try_unwrap(shared.failed_products)
            .map_err(|_| {
                anyhow::anyhow!("internal error: outstanding Arc reference to failed products")
            })?
            .into_inner();
        let final_msgs = Arc::try_unwrap(shared.failed_messages)
            .map_err(|_| {
                anyhow::anyhow!("internal error: outstanding Arc reference to failed messages")
            })?
            .into_inner();
        stats.failed_count = final_failed.len();
        stats.failed_messages = final_msgs;

        // In non-keep-going mode, return the first error after giving
        // independent products a chance to execute and be cached
        if !keep_going && !interrupted {
            let errs = Arc::try_unwrap(shared.errors)
                .map_err(|_| {
                    anyhow::anyhow!("internal error: outstanding Arc reference to errors")
                })?
                .into_inner();
            if let Some(first_err) = errs.into_iter().next() {
                return Err(first_err);
            }
        }

        Ok(stats)
    }

    /// Prepare work items for a single parallel level.
    ///
    /// Skips products with failed dependencies, takes each product's input
    /// checksum from current on-disk state, has the policy decide its action,
    /// and separates items into batch groups vs non-batch items.
    pub(super) fn prepare_level_work(
        &self,
        graph: &BuildGraph,
        level: &[usize],
        object_store: &ObjectStore,
        shared: &SharedState,
    ) -> LevelWork {
        let keep_going = self.keep_going;
        let mut work_items: Vec<WorkItem> = Vec::new();

        // First pass: identify products with failed dependencies
        let mut skipped_ids: HashSet<usize> = HashSet::new();
        {
            let failed_guard = shared.failed_products.lock();
            for &id in level {
                if super::has_failed_dependency(graph, id, &failed_guard) {
                    let product = graph.get_product(id).expect(errors::INVALID_PRODUCT_ID);
                    crate::output::detail(
                        self.verbose,
                        &format!(
                            "[{}] {} {}",
                            product.processor,
                            color::yellow("Skipping (dependency failed):"),
                            self.product_display(product)
                        ),
                    );
                    skipped_ids.insert(id);
                }
            }
        }
        if !skipped_ids.is_empty() {
            let mut failed_guard = shared.failed_products.lock();
            for id in &skipped_ids {
                failed_guard.insert(*id);
            }
        }

        // Second pass: determine work items for non-skipped products
        {
            let fp_guard = shared.failed_processors.lock();
            for &id in level {
                if skipped_ids.contains(&id) {
                    continue;
                }

                let product = graph.get_product(id).expect(errors::INVALID_PRODUCT_ID);

                // In non-keep-going mode, silently skip products from a
                // processor that failed in a previous level
                if !keep_going && fp_guard.contains(&product.processor) {
                    shared.failed_products.lock().insert(id);
                    continue;
                }
                // Take the input checksum here (per-level, in topological
                // order) rather than reusing the classify-time value. By the
                // time we reach this level, every upstream level has completed,
                // so reading inputs from disk gives the post-upstream content
                // that the cache descriptor must be keyed by — critical for
                // products whose primary inputs are produced by an upstream
                // (e.g., ipdfunite consuming marp's PDF). The classify-time
                // value can carry MISSING: for inputs that didn't exist yet,
                // which would mint a cache key the next classify can never match.
                //
                // This checksum is final: a product's inputs never include
                // its own outputs (the graph strips them), so nothing the
                // product itself writes can change it, and `handle_success`
                // caches under exactly this key.
                let input_checksum =
                    match crate::checksum::combined_input_checksum(self.build_ctx, &product.inputs)
                    {
                        Ok(cs) => cs,
                        Err(e) => {
                            if keep_going {
                                let msg = format!(
                                    "[{}] {}: {}",
                                    product.processor,
                                    self.product_display(product),
                                    e
                                );
                                // stderr: errors must survive --json, and must not
                                // corrupt the JSON event stream on stdout.
                                crate::output::error(&msg);
                                shared.failed_products.lock().insert(id);
                                shared.failed_messages.lock().push(msg);
                            } else {
                                shared.failed_products.lock().insert(id);
                                shared.errors.lock().push(e);
                            }
                            continue;
                        }
                    };

                // The one place a product's fate is decided (see `execute`).
                let action = self.policy.classify(
                    self.build_ctx,
                    product,
                    object_store,
                    &input_checksum,
                    self.force,
                );
                work_items.push(WorkItem {
                    product_id: id,
                    product: product.clone(),
                    input_checksum,
                    action,
                });
            }
        }

        // Separate work items into batch groups and non-batch items.
        // Batch groups: processor supports batch AND has >1 item that needs rebuild.
        let mut batch_groups: HashMap<String, Vec<WorkItem>> = HashMap::new();
        let mut non_batch_items: Vec<WorkItem> = Vec::new();

        // Group all items by processor name
        let mut by_processor: HashMap<String, Vec<WorkItem>> = HashMap::new();
        for item in work_items {
            let product = graph
                .get_product(item.product_id)
                .expect(errors::INVALID_PRODUCT_ID);
            by_processor
                .entry(product.processor.clone())
                .or_default()
                .push(item);
        }

        // Separate into batch vs non-batch
        // batch_size: None = disable batching, Some(0) = no limit, Some(n) = max n items
        let batching_enabled = self.batch_size.is_some();
        for (proc_name, items) in by_processor {
            let processor = self.processors.get(&proc_name);
            let supports_batch =
                processor.is_some_and(|p| effective_supports_batch(&proc_name, p.as_ref()));
            // Count items that actually need rebuild (not just cache-skip)
            let rebuild_count = items
                .iter()
                .filter(|item| item.action != ProductAction::Skip)
                .count();

            if should_batch(batching_enabled, supports_batch, rebuild_count) {
                batch_groups.insert(proc_name, items);
            } else {
                non_batch_items.extend(items);
            }
        }

        LevelWork {
            batch_groups,
            non_batch_items,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `0` means the whole group in one chunk; an explicit size wins.
    #[test]
    fn batch_chunk_size_decision_table() {
        assert_eq!(batch_chunk_size(0, 7), 7, "0 means no limit");
        assert_eq!(batch_chunk_size(3, 7), 3, "explicit size wins");
        assert_eq!(
            batch_chunk_size(3, 2),
            3,
            "size larger than group is harmless"
        );
    }

    /// `chunks(0)` panics; the sizing function must never return 0 even for
    /// degenerate inputs.
    #[test]
    fn batch_chunk_size_never_zero() {
        assert_eq!(batch_chunk_size(0, 0), 1);
    }

    /// Batching needs all three conditions; in particular a single rebuilding
    /// item must go down the non-batch path, and `batch_size: None`
    /// (`--batch-size -1`) disables batching no matter what the processor
    /// supports.
    #[test]
    fn should_batch_requires_all_conditions() {
        assert!(should_batch(true, true, 2));
        assert!(
            !should_batch(true, true, 1),
            "one rebuilding item is not a batch"
        );
        assert!(!should_batch(true, true, 0));
        assert!(
            !should_batch(true, false, 5),
            "processor must support batching"
        );
        assert!(
            !should_batch(false, true, 5),
            "batch_size None disables batching entirely"
        );
    }

    /// The no-pointer fallback must pick out exactly what a hardlink restore
    /// leaves behind — read-only *and* multiply linked — and nothing else.
    #[test]
    fn restored_hardlinks_are_read_only_and_multiply_linked() {
        let tmp = tempfile::TempDir::new().unwrap();
        let out = tmp.path().join("out");
        fs::create_dir_all(&out).unwrap();

        let object = tmp.path().join("object");
        fs::write(&object, b"cached").unwrap();
        let mut perms = fs::metadata(&object).unwrap().permissions();
        perms.set_readonly(true);
        fs::set_permissions(&object, perms).unwrap();
        let restored = out.join("restored.txt");
        fs::hard_link(&object, &restored).unwrap();

        let written = out.join("written.txt");
        fs::write(&written, b"tool output").unwrap();

        let read_only_copy = out.join("readonly.txt");
        fs::write(&read_only_copy, b"copy").unwrap();
        let mut perms = fs::metadata(&read_only_copy).unwrap().permissions();
        perms.set_readonly(true);
        fs::set_permissions(&read_only_copy, perms).unwrap();

        let found = restored_hardlinks_under(&[Arc::new(out)]);
        assert_eq!(found, vec![restored]);
    }
}
