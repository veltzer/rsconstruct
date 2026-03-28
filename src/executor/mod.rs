mod execution;
mod handlers;

use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use indicatif::ProgressBar;
use parking_lot::Mutex;

use crate::cli::DisplayOptions;
use crate::errors;
use crate::color;
use crate::graph::BuildGraph;
use crate::object_store::{ExplainAction, ObjectStore};
use crate::processors::{FailedProduct, ProcessStats, ProcessorMap};

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
    input_checksum: String,
    needs_rebuild: bool,
}

/// Context passed to handler methods for a single product operation.
/// Groups the parameters common across handle_restore, handle_error, handle_success.
struct HandlerContext<'b> {
    product: &'b crate::graph::Product,
    id: usize,
    cache_key: String,
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
}

/// Shared mutable state passed to product processing helpers.
#[derive(Debug)]
struct SharedState {
    stats: Arc<Mutex<HashMap<String, ProcessStats>>>,
    errors: Arc<Mutex<Vec<anyhow::Error>>>,
    failed_products: Arc<Mutex<HashSet<usize>>>,
    failed_messages: Arc<Mutex<Vec<String>>>,
    failed_details: Arc<Mutex<Vec<FailedProduct>>>,
    failed_processors: Arc<Mutex<HashSet<String>>>,
    unchanged_products: Arc<Mutex<HashSet<usize>>>,
    global_current: Arc<AtomicUsize>,
    global_total: usize,
}

/// Pre-build classification: count how many products will be skipped, restored, or built.
/// This is a fast read-only pass (checksums + cache lookups, no mutations).
/// Products are processed in topological order so that dependency changes propagate:
/// if a product will be rebuilt or restored, its dependents are also marked for rebuild.
pub fn classify_products(
    graph: &BuildGraph,
    order: &[usize],
    object_store: &ObjectStore,
    force: bool,
) -> (usize, usize, usize) {
    let mut skip_count = 0;
    let mut restore_count = 0;
    let mut build_count = 0;
    // Track which products will be rebuilt or restored (their dependents can't be skipped)
    let mut will_change: HashSet<usize> = HashSet::new();

    for &id in order {
        let product = graph.get_product(id).expect(errors::INVALID_PRODUCT_ID);
        let cache_key = product.cache_key();

        // If any dependency will change, this product must rebuild
        let dep_changed = graph.get_dependencies(id).iter().any(|d| will_change.contains(d));

        let input_checksum = match object_store.combined_input_checksum_fast(&product.inputs) {
            Ok(cs) => cs,
            Err(_) => {
                build_count += 1;
                will_change.insert(id);
                continue;
            }
        };

        let needs_rebuild = if let Some(ref output_dir) = product.output_dir {
            object_store.needs_rebuild_output_dir(&cache_key, &input_checksum, output_dir)
        } else {
            object_store.needs_rebuild(&cache_key, &input_checksum, &product.outputs)
        };

        let can_restore = if let Some(ref _output_dir) = product.output_dir {
            object_store.can_restore_output_dir(&cache_key, &input_checksum)
        } else {
            object_store.can_restore(&cache_key, &input_checksum, &product.outputs)
        };

        if !force && !dep_changed && !needs_rebuild {
            skip_count += 1;
        } else if !force && !dep_changed && can_restore {
            restore_count += 1;
            will_change.insert(id);
        } else {
            build_count += 1;
            will_change.insert(id);
        }
    }

    (skip_count, restore_count, build_count)
}

/// Executor handles running products through their processors
/// It respects dependency order and can parallelize independent products
pub struct Executor<'a> {
    processors: &'a ProcessorMap,
    parallel: usize,
    verbose: bool,
    display_opts: DisplayOptions,
    interrupted: Arc<AtomicBool>,
    /// Batch size setting: None = disable batching, Some(0) = no limit, Some(n) = max n files per batch
    batch_size: Option<usize>,
    /// Whether to show explain reasons for skip/restore/rebuild decisions
    explain: bool,
    /// Number of times to retry failed products (0 = no retries)
    retry: usize,
}

impl<'a> Executor<'a> {
    pub fn new(
        processors: &'a ProcessorMap,
        opts: ExecutorOptions,
        interrupted: Arc<AtomicBool>,
    ) -> Self {
        Self {
            processors,
            parallel: opts.parallel,
            verbose: opts.verbose,
            display_opts: opts.display_opts,
            interrupted,
            batch_size: opts.batch_size,
            explain: opts.explain,
            retry: opts.retry,
        }
    }

    /// Check if the build was interrupted (Ctrl+C).
    fn is_interrupted(&self) -> bool {
        self.interrupted.load(Ordering::SeqCst)
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

    /// Pre-build classification (delegates to the free function).
    fn classify_products(
        graph: &BuildGraph,
        order: &[usize],
        object_store: &ObjectStore,
        force: bool,
    ) -> (usize, usize, usize) {
        classify_products(graph, order, object_store, force)
    }

    /// Print an explain line for a product showing what action will be taken and why.
    fn print_explain(&self, product: &crate::graph::Product, action: &ExplainAction) {
        let styled = match action {
            ExplainAction::Skip => color::dim("SKIP"),
            ExplainAction::Restore(_) => color::cyan("RESTORE"),
            ExplainAction::Rebuild(_) => color::yellow("BUILD"),
        };
        println!("[{}] {} {} ({})", product.processor,
            styled,
            self.product_display(product),
            action);
    }

    /// Check if any dependency of a product has failed
    fn has_failed_dependency(&self, graph: &BuildGraph, id: usize, failed: &HashSet<usize>) -> bool {
        for &dep_id in graph.get_dependencies(id) {
            if failed.contains(&dep_id) {
                return true;
            }
        }
        false
    }

    /// Compute levels of products that can be executed in parallel
    /// Products in the same level have no dependencies on each other
    fn compute_parallel_levels(&self, graph: &BuildGraph, order: &[usize]) -> Vec<Vec<usize>> {
        let mut levels: Vec<Vec<usize>> = Vec::new();
        let mut product_level: HashMap<usize, usize> = HashMap::new();

        for &id in order {
            // Find the maximum level of all dependencies
            let max_dep_level = graph.get_dependencies(id)
                .iter()
                .filter_map(|&dep_id| product_level.get(&dep_id))
                .max()
                .copied()
                .unwrap_or(0);

            // This product goes in the next level after its dependencies
            let my_level = if graph.get_dependencies(id).is_empty() {
                0
            } else {
                max_dep_level + 1
            };

            product_level.insert(id, my_level);

            // Ensure we have enough levels
            while levels.len() <= my_level {
                levels.push(Vec::new());
            }
            levels[my_level].push(id);
        }

        levels
    }

    /// Clean all products.
    /// Returns a map of processor name → number of files removed.
    pub fn clean(&self, graph: &BuildGraph, verbose: bool) -> Result<HashMap<String, usize>> {
        let mut stats: HashMap<String, usize> = HashMap::new();
        for product in graph.products() {
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
