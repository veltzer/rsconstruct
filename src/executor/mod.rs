mod execution;
mod handlers;
mod policy;

pub use policy::{BuildPolicy, IncrementalPolicy, ProductAction};

use anyhow::Result;
use indicatif::ProgressBar;
use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
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

/// A work item representing a product to be processed in a build level.
struct WorkItem {
    product_id: usize,
    /// Taken when the product is dispatched — after every upstream product
    /// ran — and the key its outputs are cached under (see `handle_success`).
    input_checksum: String,
    /// The policy's decision at dispatch time. This, not the up-front
    /// [`Classification`], is what the executor acts on.
    action: ProductAction,
}

/// Called by the executor after each level with the products that built or
/// restored in it, before the next level is dispatched. It may grow the
/// inputs of products that have not run yet — re-analysis of a source one
/// of those products regenerated — and returns whether the graph changed,
/// in which case the executor re-links it and re-plans the remaining
/// levels.
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

/// Prepared work for a single dependency level, split into batch and non-batch items.
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
}

/// Result of [`classify_products`]: counts plus per-product actions in
/// topological order.
///
/// This is a *prediction*, made before anything runs. It sizes the progress
/// bar, feeds the "N to build" summary and decides which outputs
/// [`unlink_pending_outputs`] removes. What actually happens to each product
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
        });
    }

    Classification {
        skip_count,
        restore_count,
        build_count,
        products,
    }
}

/// Unlink the on-disk outputs of every product classified as Build or Restore.
///
/// Called once between classify and execute so that any "to-be-rebuilt" output
/// is guaranteed to be gone from disk by the time execution starts. If a
/// processor then fails (or its upstream fails and it is skipped), the stale
/// version cannot remain on disk — there is nothing to confuse the user into
/// thinking the build succeeded.
///
/// Uses the same per-product unlink logic as the pre-execute cleanup
/// ([`execution::remove_stale_outputs`]), so Creator-style products with
/// shared `output_dirs` only remove files they previously owned.
pub fn unlink_pending_outputs(
    graph: &BuildGraph,
    object_store: &ObjectStore,
    classification: &Classification,
) -> Result<()> {
    for c in &classification.products {
        if matches!(c.action, ProductAction::Skip) {
            continue;
        }
        let product = graph.get_product(c.id).expect(errors::INVALID_PRODUCT_ID);
        execution::remove_stale_outputs(product, object_store, &c.input_checksum)?;
    }
    Ok(())
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

/// Compute levels of products that can be executed in parallel
/// Products in the same level have no dependencies on each other
///
/// `order` is a topological order of the products to schedule. Dependencies
/// outside it (products that already ran, when the remaining levels are
/// re-planned mid-build) count as satisfied.
pub fn compute_parallel_levels(graph: &BuildGraph, order: &[usize]) -> Vec<Vec<usize>> {
    let mut levels: Vec<Vec<usize>> = Vec::new();
    let mut product_level: HashMap<usize, usize> = HashMap::new();

    for &id in order {
        // This product goes in the next level after its latest scheduled
        // dependency, or first when none of them is scheduled.
        let my_level = graph
            .get_dependencies(id)
            .iter()
            .filter_map(|dep_id| product_level.get(dep_id))
            .max()
            .map_or(0, |level| level + 1);

        product_level.insert(id, my_level);

        // Ensure we have enough levels
        while levels.len() <= my_level {
            levels.push(Vec::new());
        }
        levels[my_level].push(id);
    }

    levels
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Re-planning mid-build schedules only the products not yet dispatched;
    /// a dependency outside that set already ran and holds nothing back.
    #[test]
    fn parallel_levels_treat_unscheduled_dependencies_as_done() {
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

        let levels = compute_parallel_levels(&g, &[b, c]);
        assert_eq!(levels, vec![vec![b], vec![c]], "a already ran");
        assert_eq!(compute_parallel_levels(&g, &[a, b, c]).len(), 3);
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

    /// A diamond top → {left, right} → bottom must schedule as three levels
    /// with left and right side by side; an independent node always lands in
    /// level 0.
    #[test]
    fn parallel_levels_diamond() {
        let mut graph = BuildGraph::new();
        let top = graph
            .add_product(vec!["a.src".into()], vec!["a.o".into()], "cc", None)
            .unwrap();
        let left = graph
            .add_product(vec!["a.o".into()], vec!["b.o".into()], "cc", None)
            .unwrap();
        let right = graph
            .add_product(vec!["a.o".into()], vec!["c.o".into()], "cc", None)
            .unwrap();
        let bottom = graph
            .add_product(
                vec!["b.o".into(), "c.o".into()],
                vec!["d.o".into()],
                "cc",
                None,
            )
            .unwrap();
        let lone = graph
            .add_product(vec!["x.src".into()], vec!["x.o".into()], "cc", None)
            .unwrap();
        graph.resolve_dependencies();
        let order = graph.topological_sort().unwrap();

        let levels = compute_parallel_levels(&graph, &order);

        // Level membership is order-independent; compare as sorted sets.
        let sorted = |mut ids: Vec<usize>| {
            ids.sort_unstable();
            ids
        };

        assert_eq!(
            levels.len(),
            3,
            "diamond plus a free node is three levels: {levels:?}"
        );
        assert_eq!(sorted(levels[0].clone()), sorted(vec![top, lone]));
        assert_eq!(sorted(levels[1].clone()), sorted(vec![left, right]));
        assert_eq!(levels[2], vec![bottom]);
    }

    /// Every product must appear in exactly one level — a dropped product
    /// would silently never build.
    #[test]
    fn parallel_levels_cover_all_products() {
        let mut g = BuildGraph::new();
        g.add_product(vec!["a.src".into()], vec!["a.o".into()], "cc", None)
            .unwrap();
        g.add_product(vec!["a.o".into()], vec!["b.o".into()], "cc", None)
            .unwrap();
        g.add_product(vec!["free.src".into()], vec!["free.o".into()], "cc", None)
            .unwrap();
        g.resolve_dependencies();
        let order = g.topological_sort().unwrap();

        let levels = compute_parallel_levels(&g, &order);
        let mut all: Vec<usize> = levels.into_iter().flatten().collect();
        all.sort_unstable();
        assert_eq!(all, vec![0, 1, 2]);
    }

    /// Only direct dependencies count as failed here — transitive failure
    /// propagation happens level by level as each product is marked failed.
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
