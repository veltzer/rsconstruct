mod execution;
mod handlers;
mod policy;

pub use policy::{BuildPolicy, IncrementalPolicy, ProductAction};

use anyhow::Result;
use indicatif::ProgressBar;
use parking_lot::Mutex;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::color;
use crate::display::DisplayOptions;
use crate::errors;
use crate::graph::BuildGraph;
use crate::object_store::{ExplainAction, ObjectStore};
use crate::processor::ProcessorMap;
use crate::stats::ProcessStats;

/// Result of the per-item skip/restore pre-check.
enum PreCheckResult {
    /// Item was handled (skipped, restored, or failed restore). Caller should move on.
    Handled,
    /// Item needs execution. Caller should proceed with running the processor.
    NeedsExecution,
}

/// Outcome of a cache restore attempt.
enum RestoreOutcome {
    /// Product was successfully restored from cache.
    Restored,
    /// Restore failed (error already handled/reported).
    Failed,
    /// Product is not restorable; caller should proceed with execution.
    NotRestorable,
}

/// A product dispatched to a worker.
struct WorkItem {
    product_id: usize,
    /// The product as it was when dispatched. A worker runs from this copy
    /// and does not hold the graph while the tool runs, so re-analysis can
    /// update products that have not been dispatched yet in the meantime.
    product: crate::graph::Product,
    /// Taken when the product is dispatched — after every upstream product
    /// ran — and the key its outputs are cached under (see `handle_success`).
    input_checksum: String,
    /// The policy's decision at dispatch time. This, not the up-front
    /// [`Classification`], is what the executor acts on.
    action: ProductAction,
}

/// Called by the executor as products complete, with the products that
/// built or restored since the last call, before any product they feed is
/// dispatched. It may grow the inputs of products that have not run yet —
/// re-analysis of a source one of those products regenerated — and returns
/// whether the graph changed, in which case the executor re-links it and
/// recounts which products are ready.
pub type GraphRefresh<'r> = dyn FnMut(&mut BuildGraph, &[usize]) -> Result<bool> + 'r;

/// Context passed to handler methods for a single product operation.
/// Groups the parameters common across `handle_restore`, `handle_error`, `handle_success`.
struct HandlerContext<'b> {
    product: &'b crate::graph::Product,
    id: usize,
    input_checksum: &'b str,
    proc_name: &'b str,
    keep_going: bool,
    shared: &'b SharedState,
    pb: &'b ProgressBar,
}

/// Prepared work for one wave of products that became ready together,
/// split into batch and non-batch items.
struct LevelWork {
    batch_groups: HashMap<String, Vec<WorkItem>>,
    non_batch_items: Vec<WorkItem>,
}

/// Options for configuring an Executor instance.
#[derive(Debug)]
pub struct ExecutorOptions {
    pub parallel: usize,
    pub verbose: bool,
    pub display_opts: DisplayOptions,
    pub batch_size: Option<usize>,
    pub explain: bool,
    pub retry: usize,
    /// Rebuild everything, ignoring the cache.
    pub force: bool,
    /// Keep building independent products after a failure.
    pub keep_going: bool,
    /// Record per-product timings.
    pub timings: bool,
}

/// Shared mutable state passed to product processing helpers.
#[derive(Debug)]
struct SharedState {
    stats: Arc<Mutex<HashMap<String, ProcessStats>>>,
    errors: Arc<Mutex<Vec<anyhow::Error>>>,
    failed_products: Arc<Mutex<HashSet<usize>>>,
    failed_messages: Arc<Mutex<Vec<String>>>,
    failed_processors: Arc<Mutex<HashSet<String>>>,
    /// Products that built or restored in the current level — what the
    /// [`GraphRefresh`] hook gets to see. Drained after each level.
    changed: Arc<Mutex<Vec<usize>>>,
    /// Products a worker got to — skipped, restored or run, whatever the
    /// outcome. A product predicted to change that is not in here when the
    /// run ends never ran (a failed dependency, an error, Ctrl+C), and its
    /// stale outputs are removed then.
    attempted: Arc<Mutex<HashSet<usize>>>,
    global_current: Arc<AtomicUsize>,
    global_total: usize,
}

/// Per-product classification recorded by [`classify_products`].
/// `input_checksum` is empty when the checksum could not be computed
/// (which forces Build).
pub struct ClassifiedProduct {
    pub id: usize,
    pub action: ProductAction,
    pub input_checksum: String,
    /// Predicted to build because a dependency will change; the policy was
    /// not consulted.
    pub dep_changed: bool,
}

/// Result of [`classify_products`]: counts plus per-product actions in
/// topological order.
///
/// This is a *prediction*, made before anything runs. It sizes the progress
/// bar, feeds the "N to build" summary and names the products whose outputs
/// are removed at the end of the run if they never ran (see
/// `remove_outputs_never_rebuilt`). What actually happens to each product
/// is decided again when it is dispatched — see `Executor::execute`.
pub struct Classification {
    pub skip_count: usize,
    pub restore_count: usize,
    pub build_count: usize,
    pub products: Vec<ClassifiedProduct>,
}

/// Pre-build classification: predict how many products will be skipped,
/// restored, or built. A fast read-only pass (checksums + cache lookups, no
/// mutations), in topological order so that changes propagate.
///
/// The prediction is pessimistic downstream of a change: a product whose
/// dependency will build or restore is predicted to build, since its inputs
/// are about to be rewritten and their checksums taken now mean nothing.
/// The policy only sees products whose inputs are settled. At dispatch the
/// executor asks the policy again with the real inputs, so a dependency that
/// rebuilt to identical bytes still lets the product skip or restore.
pub fn classify_products(
    ctx: &crate::build_context::BuildContext,
    policy: &dyn BuildPolicy,
    graph: &BuildGraph,
    order: &[usize],
    object_store: &ObjectStore,
    force: bool,
) -> Classification {
    let mut skip_count = 0;
    let mut restore_count = 0;
    let mut build_count = 0;
    let mut will_change: HashSet<usize> = HashSet::new();
    let mut products: Vec<ClassifiedProduct> = Vec::with_capacity(order.len());

    for &id in order {
        let product = graph.get_product(id).expect(errors::INVALID_PRODUCT_ID);
        let dep_changed = graph
            .get_dependencies(id)
            .iter()
            .any(|d| will_change.contains(d));

        let Ok(input_checksum) = crate::checksum::combined_input_checksum(ctx, &product.inputs)
        else {
            build_count += 1;
            will_change.insert(id);
            products.push(ClassifiedProduct {
                id,
                action: ProductAction::Build,
                input_checksum: String::new(),
                dep_changed: false,
            });
            continue;
        };

        let action = if dep_changed {
            ProductAction::Build
        } else {
            policy.classify(ctx, product, object_store, &input_checksum, force)
        };
        match action {
            ProductAction::Skip => {
                skip_count += 1;
            }
            ProductAction::Restore => {
                restore_count += 1;
                will_change.insert(id);
            }
            ProductAction::Build => {
                build_count += 1;
                will_change.insert(id);
            }
        }
        products.push(ClassifiedProduct {
            id,
            action,
            input_checksum,
            dep_changed,
        });
    }

    Classification {
        skip_count,
        restore_count,
        build_count,
        products,
    }
}

/// Executor handles running products through their processors
/// It respects dependency order and can parallelize independent products
pub struct Executor<'a> {
    processors: &'a ProcessorMap,
    build_ctx: &'a crate::build_context::BuildContext,
    policy: &'a dyn BuildPolicy,
    parallel: usize,
    verbose: bool,
    display_opts: DisplayOptions,
    batch_size: Option<usize>,
    explain: bool,
    retry: usize,
    force: bool,
    keep_going: bool,
    timings: bool,
}

impl<'a> Executor<'a> {
    pub fn new(
        processors: &'a ProcessorMap,
        build_ctx: &'a crate::build_context::BuildContext,
        policy: &'a dyn BuildPolicy,
        opts: ExecutorOptions,
    ) -> Self {
        Self {
            processors,
            build_ctx,
            policy,
            parallel: opts.parallel,
            // Verbose progress lines are human output: under --json (or
            // --quiet) they would corrupt the machine-readable stream, so
            // verbosity is forced off there rather than gated at each of
            // the half-dozen println! sites.
            verbose: opts.verbose && crate::json_output::human_output_enabled(),
            display_opts: opts.display_opts,
            batch_size: opts.batch_size,
            explain: opts.explain,
            retry: opts.retry,
            force: opts.force,
            keep_going: opts.keep_going,
            timings: opts.timings,
        }
    }

    /// Check if the build was interrupted (Ctrl+C).
    fn is_interrupted(&self) -> bool {
        self.build_ctx.is_interrupted()
    }

    /// Display a product with the current display options.
    fn product_display(&self, product: &crate::graph::Product) -> String {
        product.display(self.display_opts)
    }

    /// Increment the global product counter only (no progress bar advancement).
    fn inc_global(shared: &SharedState) {
        shared.global_current.fetch_add(1, Ordering::SeqCst);
    }

    /// Increment both the progress bar and the global product counter.
    fn inc_progress(pb: &ProgressBar, shared: &SharedState) {
        Self::inc_global(shared);
        pb.inc(1);
    }

    /// Print an explain line for a product showing what action will be taken and why.
    fn print_explain(&self, product: &crate::graph::Product, action: &ExplainAction) {
        let styled = match action {
            ExplainAction::Skip => color::dim("SKIP"),
            ExplainAction::Restore(_) => color::cyan("RESTORE"),
            ExplainAction::Rebuild(_) => color::yellow("BUILD"),
        };
        crate::output::info(&format!(
            "[{}] {} {} ({})",
            product.processor,
            styled,
            self.product_display(product),
            action
        ));
    }

    /// Clean all products.
    /// Returns a map of processor name → number of files removed.
    pub fn clean(&self, graph: &BuildGraph, verbose: bool) -> Result<HashMap<String, usize>> {
        let mut stats: HashMap<String, usize> = HashMap::new();
        for product in graph.products() {
            // Nothing else in this serial loop observes the flag; without
            // this check a clean over a large graph cannot be Ctrl+C'd.
            if self.is_interrupted() {
                return Err(crate::exit_code::interrupted());
            }
            if let Some(processor) = self.processors.get(&product.processor) {
                let count = processor.clean(product, verbose)?;
                if count > 0 {
                    *stats.entry(product.processor.clone()).or_default() += count;
                }
            }
        }
        Ok(stats)
    }
}

/// Check if any dependency of a product has failed
pub fn has_failed_dependency(graph: &BuildGraph, id: usize, failed: &HashSet<usize>) -> bool {
    for &dep_id in graph.get_dependencies(id) {
        if failed.contains(&dep_id) {
            return true;
        }
    }
    false
}

/// Where a product is in the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProductState {
    /// Some dependency has not finished.
    Waiting,
    /// Handed out by [`ReadyTracker::take_ready`]; not finished yet.
    Dispatched,
    /// Finished, whatever the outcome.
    Done,
}

/// Which products may run now: each product's count of unfinished
/// dependencies, and the products whose count reached zero.
///
/// This replaces scheduling by levels. A product is ready the moment its
/// last dependency finishes, not when everything in an earlier level has —
/// a level was a barrier, and one slow product held up the whole next one.
struct ReadyTracker {
    unfinished_deps: Vec<usize>,
    state: Vec<ProductState>,
    /// Ready, not yet handed out. Ordered by product id, so a single worker
    /// runs products in a stable order.
    ready: BTreeSet<usize>,
    /// Handed out and not yet finished.
    in_flight: usize,
}

impl ReadyTracker {
    fn new(graph: &BuildGraph) -> Self {
        let count = graph.products().len();
        let mut tracker = Self {
            unfinished_deps: vec![0; count],
            state: vec![ProductState::Waiting; count],
            ready: BTreeSet::new(),
            in_flight: 0,
        };
        tracker.recount(graph);
        tracker
    }

    /// Recompute every waiting product's count from the graph — after the
    /// graph gained edges mid-build (re-analysis), a product that looked
    /// ready may now wait for a product that has not run.
    fn recount(&mut self, graph: &BuildGraph) {
        self.ready.clear();
        for id in 0..self.state.len() {
            if self.state[id] != ProductState::Waiting {
                continue;
            }
            self.unfinished_deps[id] = graph
                .get_dependencies(id)
                .iter()
                .filter(|&&dep| self.state[dep] != ProductState::Done)
                .count();
            if self.unfinished_deps[id] == 0 {
                self.ready.insert(id);
            }
        }
    }

    /// Hand out every ready product.
    fn take_ready(&mut self) -> Vec<usize> {
        let ready: Vec<usize> = std::mem::take(&mut self.ready).into_iter().collect();
        for &id in &ready {
            self.state[id] = ProductState::Dispatched;
        }
        self.in_flight += ready.len();
        ready
    }

    /// Record that a handed-out product finished; its dependents with no
    /// unfinished dependency left become ready.
    fn finish(&mut self, graph: &BuildGraph, id: usize) {
        debug_assert_eq!(self.state[id], ProductState::Dispatched);
        self.state[id] = ProductState::Done;
        self.in_flight -= 1;
        for &dependent in graph.get_dependents(id) {
            if self.state[dependent] == ProductState::Waiting {
                self.unfinished_deps[dependent] -= 1;
                if self.unfinished_deps[dependent] == 0 {
                    self.ready.insert(dependent);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A product is ready the moment its own dependencies finish, whatever
    /// else is still running: in a diamond plus a slow independent chain,
    /// `left` and `right` become ready as soon as `top` finishes, while
    /// `slow` is still in flight — no level barrier.
    #[test]
    fn ready_tracker_releases_dependents_without_a_barrier() {
        let mut g = BuildGraph::new();
        let top = g
            .add_product(vec!["a.src".into()], vec!["a.o".into()], "cc", None)
            .unwrap();
        let left = g
            .add_product(vec!["a.o".into()], vec!["b.o".into()], "cc", None)
            .unwrap();
        let right = g
            .add_product(vec!["a.o".into()], vec!["c.o".into()], "cc", None)
            .unwrap();
        let bottom = g
            .add_product(
                vec!["b.o".into(), "c.o".into()],
                vec!["d.o".into()],
                "cc",
                None,
            )
            .unwrap();
        let slow = g
            .add_product(vec!["x.src".into()], vec!["x.o".into()], "cc", None)
            .unwrap();
        g.resolve_dependencies();

        let mut tracker = ReadyTracker::new(&g);
        assert_eq!(tracker.take_ready(), vec![top, slow]);
        tracker.finish(&g, top);
        assert_eq!(tracker.take_ready(), vec![left, right], "slow still runs");
        tracker.finish(&g, left);
        assert_eq!(
            tracker.take_ready(),
            Vec::<usize>::new(),
            "bottom needs right"
        );
        tracker.finish(&g, right);
        assert_eq!(tracker.take_ready(), vec![bottom]);
        tracker.finish(&g, bottom);
        assert_eq!(tracker.in_flight, 1, "slow is still in flight");
        tracker.finish(&g, slow);
        assert_eq!(tracker.in_flight, 0);
        assert_eq!(tracker.take_ready(), Vec::<usize>::new());
    }

    /// Edges added mid-build (re-analysis found a generated input) make a
    /// ready product wait again for a producer that has not run.
    #[test]
    fn ready_tracker_recount_respects_new_edges() {
        let mut g = BuildGraph::new();
        let producer = g
            .add_product(vec!["gen.src".into()], vec!["gen.h".into()], "gen", None)
            .unwrap();
        let blocker = g
            .add_product(vec!["b.src".into()], vec!["b.o".into()], "cc", None)
            .unwrap();
        let consumer = g
            .add_product(vec!["main.c".into()], vec!["main.o".into()], "cc", None)
            .unwrap();
        g.resolve_dependencies();

        // All three are ready while nothing links them.
        let mut tracker = ReadyTracker::new(&g);
        assert_eq!(
            tracker.ready.iter().copied().collect::<Vec<_>>(),
            vec![producer, blocker, consumer]
        );

        // Re-analysis finds that the consumer reads the producer's output.
        g.add_inputs(consumer, &["gen.h".into()]);
        g.resolve_dependencies();
        tracker.recount(&g);
        assert_eq!(
            tracker.take_ready(),
            vec![producer, blocker],
            "consumer waits for gen.h"
        );
        tracker.finish(&g, producer);
        assert_eq!(tracker.take_ready(), vec![consumer]);
    }

    /// The up-front prediction is pessimistic downstream of a change: a
    /// product whose dependency will build is predicted to build even when
    /// its own cache entry matches, since its inputs are about to change.
    /// (At dispatch the policy decides again on the real inputs.)
    #[test]
    fn classify_predicts_build_downstream_of_a_change() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = ObjectStore::new_in(tmp.path());
        let ctx = crate::build_context::BuildContext::new();
        // The mtime cache is CWD-relative; keep parallel tests apart.
        ctx.set_mtime_check(false);
        let src = tmp.path().join("a.src");
        let mid = tmp.path().join("a.mid");
        std::fs::write(&src, "a").unwrap();
        std::fs::write(&mid, "m").unwrap();

        let mut g = BuildGraph::new();
        let producer = g
            .add_product(vec![src], vec![mid.clone()], "gen", None)
            .unwrap();
        let checker = g
            .add_product(vec![mid.clone()], vec![], "check", None)
            .unwrap();
        g.resolve_dependencies();

        // The checker has a matching PASS marker; the producer has nothing
        // cached, so it builds.
        let mid_checksum =
            crate::checksum::combined_input_checksum(&ctx, std::slice::from_ref(&mid)).unwrap();
        let marker_key = g
            .get_product(checker)
            .unwrap()
            .descriptor_key(&mid_checksum);
        store.store_marker(&ctx, &marker_key).unwrap();

        let order = g.topological_sort().unwrap();
        let classification = classify_products(&ctx, &IncrementalPolicy, &g, &order, &store, false);
        let action_of = |id: usize| {
            classification
                .products
                .iter()
                .find(|c| c.id == id)
                .map(|c| c.action)
        };
        assert_eq!(action_of(producer), Some(ProductAction::Build));
        assert_eq!(action_of(checker), Some(ProductAction::Build));
        assert_eq!(
            IncrementalPolicy.classify(
                &ctx,
                g.get_product(checker).unwrap(),
                &store,
                &mid_checksum,
                false
            ),
            ProductAction::Skip,
            "the policy alone, on unchanged inputs, would skip it"
        );
    }

    /// Every product is handed out exactly once — a dropped product would
    /// silently never build.
    #[test]
    fn ready_tracker_hands_out_every_product_once() {
        let mut g = BuildGraph::new();
        g.add_product(vec!["a.src".into()], vec!["a.o".into()], "cc", None)
            .unwrap();
        g.add_product(vec!["a.o".into()], vec!["b.o".into()], "cc", None)
            .unwrap();
        g.add_product(vec!["free.src".into()], vec!["free.o".into()], "cc", None)
            .unwrap();
        g.resolve_dependencies();

        let mut tracker = ReadyTracker::new(&g);
        let mut handed_out: Vec<usize> = Vec::new();
        loop {
            let wave = tracker.take_ready();
            if wave.is_empty() {
                break;
            }
            for &id in &wave {
                tracker.finish(&g, id);
            }
            handed_out.extend(wave);
        }
        handed_out.sort_unstable();
        assert_eq!(handed_out, vec![0, 1, 2]);
    }

    /// Only direct dependencies count as failed here — transitive failure
    /// propagates as each skipped product is itself marked failed before its
    /// dependents are dispatched.
    #[test]
    fn failed_dependency_is_direct_only() {
        let mut g = BuildGraph::new();
        let a = g
            .add_product(vec!["a.src".into()], vec!["a.o".into()], "cc", None)
            .unwrap();
        let b = g
            .add_product(vec!["a.o".into()], vec!["b.o".into()], "cc", None)
            .unwrap();
        let c = g
            .add_product(vec!["b.o".into()], vec!["c.o".into()], "cc", None)
            .unwrap();
        g.resolve_dependencies();

        let failed: HashSet<usize> = [a].into();
        assert!(
            has_failed_dependency(&g, b, &failed),
            "b directly depends on failed a"
        );
        assert!(
            !has_failed_dependency(&g, c, &failed),
            "c depends on a only through b; direct check must not see it"
        );
        assert!(
            !has_failed_dependency(&g, a, &failed),
            "a has no dependencies"
        );
    }
}
