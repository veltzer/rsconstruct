use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use indicatif::{ProgressBar, ProgressStyle};
use parking_lot::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use crate::cli::DisplayOptions;
use crate::color;
use crate::graph::BuildGraph;
use crate::json_output::{self, emit_product_complete, ProductStatus};
use crate::object_store::{ExplainAction, ObjectStore};
use crate::processors::{BuildStats, ProcessStats, ProductDiscovery, ProductTiming};

/// Options for configuring an Executor instance.
#[derive(Debug)]
pub struct ExecutorOptions {
    pub parallel: usize,
    pub verbose: bool,
    pub display_opts: DisplayOptions,
    pub batch_size: Option<usize>,
    pub progress: bool,
    pub explain: bool,
}

/// Shared mutable state passed to product processing helpers.
#[derive(Debug)]
struct SharedState {
    stats: Arc<Mutex<HashMap<String, ProcessStats>>>,
    errors: Arc<Mutex<Vec<anyhow::Error>>>,
    failed_products: Arc<Mutex<HashSet<usize>>>,
    failed_messages: Arc<Mutex<Vec<String>>>,
    failed_processors: Arc<Mutex<HashSet<String>>>,
    unchanged_products: Arc<Mutex<HashSet<usize>>>,
}

/// Executor handles running products through their processors
/// It respects dependency order and can parallelize independent products
pub struct Executor<'a> {
    processors: &'a HashMap<String, Box<dyn ProductDiscovery>>,
    parallel: usize,
    verbose: bool,
    display_opts: DisplayOptions,
    interrupted: Arc<AtomicBool>,
    /// Batch size setting: None = disable batching, Some(0) = no limit, Some(n) = max n files per batch
    batch_size: Option<usize>,
    /// Whether to show a progress bar
    progress: bool,
    /// Whether to show explain reasons for skip/restore/rebuild decisions
    explain: bool,
}

impl<'a> Executor<'a> {
    pub fn new(
        processors: &'a HashMap<String, Box<dyn ProductDiscovery>>,
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
            progress: opts.progress,
            explain: opts.explain,
        }
    }

    /// Display a product with the current display options.
    fn product_display(&self, product: &crate::graph::Product) -> String {
        product.display(self.display_opts)
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

    /// Handle the "skip (unchanged)" case for a product.
    /// Logs, emits JSON event, increments stats and progress bar.
    fn handle_skip(
        &self,
        product: &crate::graph::Product,
        shared: &SharedState,
        pb_ref: &ProgressBar,
    ) {
        if self.verbose {
            println!("[{}] {} {}", product.processor,
                color::dim("Skipping (unchanged):"),
                self.product_display(product));
        }
        emit_product_complete(
            &self.product_display(product),
            &product.processor,
            ProductStatus::Skipped,
            None,
            None,
        );
        let mut stats = shared.stats.lock();
        let proc_stats = stats
            .entry(product.processor.clone())
            .or_default();
        proc_stats.skipped += 1;
        pb_ref.inc(1);
    }

    /// Handle cache restore for a product.
    /// Returns `Some(true)` if restored, `Some(false)` if restore failed (error handled),
    /// `None` if not restorable (caller should proceed with execution).
    /// When `emit_fail_event` is true, emits a product_complete "failed" JSON event on error.
    #[allow(clippy::too_many_arguments)]
    fn handle_restore(
        &self,
        product: &crate::graph::Product,
        id: usize,
        object_store: &ObjectStore,
        cache_key: &str,
        input_checksum: &str,
        force: bool,
        keep_going: bool,
        emit_fail_event: bool,
        shared: &SharedState,
        pb_ref: &ProgressBar,
    ) -> Option<bool> {
        if force {
            return None;
        }
        let restore_result = object_store.restore_from_cache(cache_key, input_checksum, &product.outputs);
        match restore_result {
            Ok(true) => {
                if self.verbose {
                    println!("[{}] {} {}", product.processor,
                        color::cyan("Restored from cache:"),
                        self.product_display(product));
                }
                emit_product_complete(
                    &self.product_display(product),
                    &product.processor,
                    ProductStatus::Restored,
                    None,
                    None,
                );
                let mut stats = shared.stats.lock();
                let proc_stats = stats
                    .entry(product.processor.clone())
                    .or_default();
                proc_stats.restored += 1;
                proc_stats.files_restored += product.outputs.len();
                pb_ref.inc(1);
                Some(true)
            }
            Err(e) => {
                if emit_fail_event {
                    emit_product_complete(
                        &self.product_display(product),
                        &product.processor,
                        ProductStatus::Failed,
                        None,
                        Some(&e.to_string()),
                    );
                }
                if keep_going {
                    let msg = format!("[{}] {}: {}", product.processor, self.product_display(product), e);
                    println!("{}", color::red(&format!("Error: {}", msg)));
                    shared.failed_products.lock().insert(id);
                    shared.failed_messages.lock().push(msg);
                } else {
                    shared.failed_products.lock().insert(id);
                    shared.errors.lock().push(e);
                }
                pb_ref.inc(1);
                Some(false)
            }
            Ok(false) => None,
        }
    }

    /// Handle a product execution error.
    /// Emits a JSON event, records failure stats, and records keep-going / non-keep-going state.
    #[allow(clippy::too_many_arguments)]
    fn handle_error(
        &self,
        product: &crate::graph::Product,
        id: usize,
        proc_name: &str,
        error: anyhow::Error,
        duration: Option<Duration>,
        keep_going: bool,
        shared: &SharedState,
    ) {
        emit_product_complete(
            &self.product_display(product),
            &product.processor,
            ProductStatus::Failed,
            duration,
            Some(&error.to_string()),
        );
        {
            let mut stats = shared.stats.lock();
            let proc_stats = stats
                .entry(proc_name.to_string())
                .or_default();
            proc_stats.failed += 1;
        }
        if keep_going {
            let msg = format!("[{}] {}: {}", proc_name, self.product_display(product), error);
            println!("{}", color::red(&format!("Error: {}", msg)));
            shared.failed_products.lock().insert(id);
            shared.failed_messages.lock().push(msg);
        } else {
            shared.failed_products.lock().insert(id);
            shared.failed_processors.lock().insert(proc_name.to_string());
            shared.errors.lock().push(error);
        }
    }

    /// Handle caching outputs and recording stats after successful execution.
    /// Returns `true` if caching succeeded, `false` if it failed (error is handled internally).
    /// On success, emits a product_complete "success" JSON event and increments processed/files_created.
    #[allow(clippy::too_many_arguments)]
    fn handle_success(
        &self,
        product: &crate::graph::Product,
        id: usize,
        object_store: &ObjectStore,
        cache_key: &str,
        input_checksum: &str,
        proc_name: &str,
        duration: Option<Duration>,
        keep_going: bool,
        shared: &SharedState,
        pb_ref: &ProgressBar,
    ) -> bool {
        match object_store.cache_outputs(cache_key, input_checksum, &product.outputs) {
            Ok(changed) => {
                if !changed {
                    shared.unchanged_products.lock().insert(id);
                }
            }
            Err(e) => {
                emit_product_complete(
                    &self.product_display(product),
                    &product.processor,
                    ProductStatus::Failed,
                    duration,
                    Some(&e.to_string()),
                );
                if keep_going {
                    let msg = format!("[{}] {}: {}", product.processor, self.product_display(product), e);
                    println!("{}", color::red(&format!("Error: {}", msg)));
                    shared.failed_products.lock().insert(id);
                    shared.failed_messages.lock().push(msg);
                } else {
                    shared.failed_products.lock().insert(id);
                    shared.errors.lock().push(e);
                }
                pb_ref.inc(1);
                return false;
            }
        }
        emit_product_complete(
            &self.product_display(product),
            &product.processor,
            ProductStatus::Success,
            duration,
            None,
        );
        let mut stats = shared.stats.lock();
        let proc_stats = stats
            .entry(proc_name.to_string())
            .or_default();
        proc_stats.processed += 1;
        proc_stats.files_created += product.outputs.len();
        true
    }

    /// Execute all products in the graph that need rebuilding
    pub fn execute(
        &self,
        graph: &BuildGraph,
        object_store: &ObjectStore,
        force: bool,
        timings: bool,
        keep_going: bool,
    ) -> Result<BuildStats> {
        let build_start = Instant::now();
        let order = graph.topological_sort()?;

        // Emit JSON build start event
        json_output::emit_build_start(order.len());

        let result = self.execute_parallel(graph, &order, object_store, force, timings, keep_going);

        match result {
            Ok(mut stats) => {
                stats.total_duration = build_start.elapsed();

                // Emit JSON build summary
                json_output::emit_build_summary(
                    stats.total_processed() + stats.total_skipped() + stats.total_restored() + stats.failed_count,
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

    /// Execute products in parallel where dependencies allow.
    /// Within each level, batch-supporting processors with multiple items
    /// are grouped and executed via execute_batch() in a single thread.
    fn execute_parallel(
        &self,
        graph: &BuildGraph,
        order: &[usize],
        object_store: &ObjectStore,
        force: bool,
        timings: bool,
        keep_going: bool,
    ) -> Result<BuildStats> {
        // Group products into levels that can run in parallel
        let levels = self.compute_parallel_levels(graph, order);

        // Count total products per processor for progress display
        let mut total_per_processor: HashMap<String, usize> = HashMap::new();
        for &product_id in order {
            let product = graph.get_product(product_id).expect("internal error: invalid product id");
            *total_per_processor.entry(product.processor.clone()).or_insert(0) += 1;
        }
        let total_per_processor = Arc::new(total_per_processor);
        let current_per_processor: Arc<Mutex<HashMap<String, usize>>> =
            Arc::new(Mutex::new(HashMap::new()));

        // Create progress bar (only shown when --progress is passed, hidden in JSON mode)
        let pb = if !self.progress || json_output::is_json_mode() {
            ProgressBar::hidden()
        } else {
            let pb = ProgressBar::new(order.len() as u64);
            pb.set_style(
                ProgressStyle::default_bar()
                    .template("[{elapsed_precise}] {bar:40} {pos}/{len} {msg}")
                    .expect("internal error: invalid progress bar template")
                    .progress_chars("=> "),
            );
            pb
        };

        let stats_by_processor: Arc<Mutex<HashMap<String, ProcessStats>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let errors: Arc<Mutex<Vec<anyhow::Error>>> = Arc::new(Mutex::new(Vec::new()));
        let failed_products: Arc<Mutex<HashSet<usize>>> = Arc::new(Mutex::new(HashSet::new()));
        let failed_messages: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let failed_processors: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
        let unchanged_products: Arc<Mutex<HashSet<usize>>> = Arc::new(Mutex::new(HashSet::new()));

        for level in levels {
            // Check for Ctrl+C before starting next level
            if self.interrupted.load(Ordering::SeqCst) {
                break;
            }

            // Determine which products in this level need work
            // Each work item: (product_id, input_checksum, needs_rebuild)
            let mut work_items: Vec<(usize, String, bool)> = Vec::new();

            // First pass: identify products with failed dependencies
            let mut skipped_ids: HashSet<usize> = HashSet::new();
            {
                let failed_guard = failed_products.lock();
                for &id in &level {
                    if self.has_failed_dependency(graph, id, &failed_guard) {
                        let product = graph.get_product(id).expect("internal error: invalid product id");
                        if self.verbose {
                            println!("[{}] {} {}", product.processor,
                                color::yellow("Skipping (dependency failed):"),
                                self.product_display(product));
                        }
                        skipped_ids.insert(id);
                    }
                }
            }
            if !skipped_ids.is_empty() {
                let mut failed_guard = failed_products.lock();
                for id in &skipped_ids {
                    failed_guard.insert(*id);
                }
            }

            // Second pass: determine work items for non-skipped products
            {
                let fp_guard = failed_processors.lock();
                for &id in &level {
                    if skipped_ids.contains(&id) {
                        continue;
                    }

                    let product = graph.get_product(id).expect("internal error: invalid product id");

                    // In non-keep-going mode, silently skip products from a
                    // processor that failed in a previous level
                    if !keep_going && fp_guard.contains(&product.processor) {
                        failed_products.lock().insert(id);
                        continue;
                    }
                    let cache_key = product.cache_key();

                    // Early cutoff: if all dependencies produced identical outputs,
                    // reuse the cached input checksum instead of recomputing
                    let deps = graph.get_dependencies(id);
                    let input_checksum = {
                        let unchanged_guard = unchanged_products.lock();
                        let all_deps_unchanged = !deps.is_empty()
                            && deps.iter().all(|d| unchanged_guard.contains(d));
                        if all_deps_unchanged {
                            match object_store.get_cached_input_checksum(&cache_key) {
                                Some(cs) => Ok(cs),
                                None => object_store.combined_input_checksum_fast(&product.inputs),
                            }
                        } else {
                            object_store.combined_input_checksum_fast(&product.inputs)
                        }
                    };

                    let input_checksum = match input_checksum {
                        Ok(cs) => cs,
                        Err(e) => {
                            if keep_going {
                                let msg = format!("[{}] {}: {}", product.processor, self.product_display(product), e);
                                println!("{}", color::red(&format!("Error: {}", msg)));
                                failed_products.lock().insert(id);
                                failed_messages.lock().push(msg);
                            } else {
                                failed_products.lock().insert(id);
                                errors.lock().push(e);
                            }
                            continue;
                        }
                    };

                    let needs = force || object_store.needs_rebuild(&cache_key, &input_checksum, &product.outputs);
                    work_items.push((id, input_checksum, needs));
                }
            }

            // Separate work items into batch groups and non-batch items.
            // Batch groups: processor supports batch AND has >1 item that needs rebuild.
            let mut batch_groups: HashMap<String, Vec<(usize, String, bool)>> = HashMap::new();
            let mut non_batch_items: Vec<(usize, String, bool)> = Vec::new();

            // First, group all items by processor name
            let mut by_processor: HashMap<String, Vec<(usize, String, bool)>> = HashMap::new();
            for item in work_items {
                let product = graph.get_product(item.0).expect("internal error: invalid product id");
                by_processor.entry(product.processor.clone()).or_default().push(item);
            }

            // Then separate into batch vs non-batch
            // batch_size: None = disable batching, Some(0) = no limit, Some(n) = max n items
            let batching_enabled = self.batch_size.is_some();
            for (proc_name, items) in by_processor {
                let processor = self.processors.get(&proc_name);
                let supports_batch = processor.is_some_and(|p| p.supports_batch());
                // Count items that actually need rebuild (not just cache-skip)
                let rebuild_count = items.iter().filter(|(_, _, needs)| *needs).count();

                if batching_enabled && supports_batch && rebuild_count > 1 {
                    batch_groups.insert(proc_name, items);
                } else {
                    non_batch_items.extend(items);
                }
            }

            // Process this level in parallel using thread pool
            thread::scope(|s| {
                let stats_ref = &stats_by_processor;
                let errors_ref = &errors;
                let failed_ref = &failed_products;
                let failed_msgs_ref = &failed_messages;
                let failed_procs_ref = &failed_processors;
                let interrupted_ref = &self.interrupted;
                let pb_ref = &pb;

                // Spawn one thread per batch group
                let unchanged_ref = &unchanged_products;
                for (proc_name, items) in &batch_groups {
                    let shared = SharedState {
                        stats: Arc::clone(stats_ref),
                        errors: Arc::clone(errors_ref),
                        failed_products: Arc::clone(failed_ref),
                        failed_messages: Arc::clone(failed_msgs_ref),
                        failed_processors: Arc::clone(failed_procs_ref),
                        unchanged_products: Arc::clone(unchanged_ref),
                    };

                    s.spawn(move || {
                        if interrupted_ref.load(Ordering::SeqCst) {
                            return;
                        }

                        let processor = match self.processors.get(proc_name) {
                            Some(p) => p,
                            None => return,
                        };

                        // Handle skip/restore for items that don't need rebuild
                        let mut to_execute: Vec<&(usize, String, bool)> = Vec::new();
                        for item in items {
                            let (id, input_checksum, needs_rebuild) = item;
                            let product = graph.get_product(*id).expect("internal error: invalid product id");
                            let cache_key = product.cache_key();

                            if self.explain {
                                let action = object_store.explain_action(&cache_key, input_checksum, &product.outputs, force);
                                self.print_explain(product, &action);
                            }

                            if !needs_rebuild {
                                self.handle_skip(product, &shared, pb_ref);
                                continue;
                            }

                            // Try to restore from cache (batch path: no emit on failure)
                            match self.handle_restore(product, *id, object_store, &cache_key, input_checksum, force, keep_going, false, &shared, pb_ref) {
                                Some(true) => continue,  // restored
                                Some(false) => continue, // failed (error handled)
                                None => {}               // not restorable, proceed
                            }

                            to_execute.push(item);
                        }

                        if to_execute.is_empty() || interrupted_ref.load(Ordering::SeqCst) {
                            return;
                        }

                        // Determine chunk size: 0 means no limit
                        let chunk_size = match self.batch_size {
                            Some(0) | None => to_execute.len(),
                            Some(n) => n,
                        };

                        // Process in chunks
                        for chunk in to_execute.chunks(chunk_size) {
                            if interrupted_ref.load(Ordering::SeqCst) {
                                break;
                            }

                            // Execute batch chunk
                            let product_refs: Vec<&crate::graph::Product> = chunk.iter()
                                .map(|(id, _, _)| graph.get_product(*id).expect("internal error: invalid product id"))
                                .collect();

                            let displays: Vec<String> = product_refs.iter()
                                .map(|p| self.product_display(p))
                                .collect();
                            println!("[{}] {} {} files: {}",
                                proc_name,
                                color::green("Processing batch:"),
                                product_refs.len(),
                                displays.join(", "));

                            let batch_start = Instant::now();
                            let results = processor.execute_batch(&product_refs);
                            let batch_duration = batch_start.elapsed();

                            // Process per-product results
                            for (item, result) in chunk.iter().zip(results) {
                                let (id, input_checksum, _) = item;
                                let product = graph.get_product(*id).expect("internal error: invalid product id");
                                let cache_key = product.cache_key();

                                match result {
                                    Ok(()) => {
                                        if !self.handle_success(product, *id, object_store, &cache_key, input_checksum, proc_name, None, keep_going, &shared, pb_ref) {
                                            // cache_outputs failed and error was handled
                                            continue;
                                        }
                                    }
                                    Err(e) => {
                                        self.handle_error(product, *id, proc_name, e, None, keep_going, &shared);
                                    }
                                }
                                pb_ref.inc(1);
                            }

                            // Record batch timing for this chunk
                            if timings {
                                let mut stats = shared.stats.lock();
                                let proc_stats = stats
                                    .entry(proc_name.clone())
                                    .or_default();
                                proc_stats.duration += batch_duration;
                                proc_stats.product_timings.push(ProductTiming {
                                    display: format!("batch ({} files)", product_refs.len()),
                                    processor: proc_name.clone(),
                                    duration: batch_duration,
                                });
                            }
                        }
                    });
                }

                // Spawn threads for non-batch items (chunked as before)
                if !non_batch_items.is_empty() {
                    let chunk_size = non_batch_items.len().div_ceil(self.parallel);
                    let chunks: Vec<_> = non_batch_items.chunks(chunk_size.max(1)).collect();
                    let total_ref = &total_per_processor;
                    let current_ref = &current_per_processor;

                    for chunk in chunks {
                        let shared = SharedState {
                            stats: Arc::clone(stats_ref),
                            errors: Arc::clone(errors_ref),
                            failed_products: Arc::clone(failed_ref),
                            failed_messages: Arc::clone(failed_msgs_ref),
                            failed_processors: Arc::clone(failed_procs_ref),
                            unchanged_products: Arc::clone(unchanged_ref),
                        };
                        let total_ref = Arc::clone(total_ref);
                        let current_ref = Arc::clone(current_ref);

                        s.spawn(move || {
                            for (id, input_checksum, needs_rebuild) in chunk {
                                // Stop if interrupted or if there's an error (non-keep-going mode)
                                if interrupted_ref.load(Ordering::SeqCst)
                                    || (!keep_going && !shared.errors.lock().is_empty())
                                {
                                    break;
                                }

                                let product = graph.get_product(*id).expect("internal error: invalid product id");
                                let cache_key = product.cache_key();

                                if self.explain {
                                    let action = object_store.explain_action(&cache_key, input_checksum, &product.outputs, force);
                                    self.print_explain(product, &action);
                                }

                                if !needs_rebuild {
                                    self.handle_skip(product, &shared, pb_ref);
                                    continue;
                                }

                                // Try to restore from cache (non-batch path: emit on failure)
                                match self.handle_restore(product, *id, object_store, &cache_key, input_checksum, force, keep_going, true, &shared, pb_ref) {
                                    Some(true) => continue,  // restored
                                    Some(false) => continue, // failed (error handled)
                                    None => {}               // not restorable, proceed
                                }

                                if let Some(processor) = self.processors.get(&product.processor) {
                                    // Update progress counter
                                    let current = {
                                        let mut current_guard = current_ref.lock();
                                        let c = current_guard.entry(product.processor.clone()).or_insert(0);
                                        *c += 1;
                                        *c
                                    };
                                    let total = total_ref.get(&product.processor).copied().unwrap_or(1);

                                    let variant_tag = product.variant.as_ref()
                                        .map(|v| format!(":{}", v))
                                        .unwrap_or_default();
                                    println!("[{}{}] ({}/{}) {} {}", product.processor, variant_tag,
                                        current, total,
                                        color::green("Processing:"),
                                        self.product_display(product));

                                    let product_start = Instant::now();
                                    match processor.execute(product) {
                                        Ok(()) => {
                                            let duration = product_start.elapsed();

                                            if !self.handle_success(product, *id, object_store, &cache_key, input_checksum, &product.processor, Some(duration), keep_going, &shared, pb_ref) {
                                                // cache_outputs failed and error was handled
                                                continue;
                                            }

                                            // Record per-product duration (non-batch only)
                                            {
                                                let mut stats = shared.stats.lock();
                                                let proc_stats = stats
                                                    .entry(product.processor.clone())
                                                    .or_default();
                                                proc_stats.duration += duration;
                                                if timings {
                                                    proc_stats.product_timings.push(ProductTiming {
                                                        display: self.product_display(product),
                                                        processor: product.processor.clone(),
                                                        duration,
                                                    });
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            let duration = product_start.elapsed();
                                            self.handle_error(product, *id, &product.processor, e, Some(duration), keep_going, &shared);
                                        }
                                    }
                                    pb_ref.inc(1);
                                }
                            }
                        });
                    }
                }
            });

            // If interrupted, stop processing further levels
            if self.interrupted.load(Ordering::SeqCst) {
                println!("{}", color::yellow("Interrupted, saving progress..."));
                break;
            }

            // In non-keep-going mode, stop after level with errors
            if !keep_going && !errors.lock().is_empty() {
                break;
            }
        }

        pb.finish_and_clear();

        // Build aggregated stats
        let final_stats = Arc::try_unwrap(stats_by_processor)
            .map_err(|_| anyhow::anyhow!("internal error: outstanding Arc reference to stats"))?
            .into_inner();
        let mut stats = BuildStats::default();
        for (_, proc_stats) in final_stats {
            stats.add(proc_stats);
        }

        let final_failed = Arc::try_unwrap(failed_products)
            .map_err(|_| anyhow::anyhow!("internal error: outstanding Arc reference to failed products"))?
            .into_inner();
        let final_msgs = Arc::try_unwrap(failed_messages)
            .map_err(|_| anyhow::anyhow!("internal error: outstanding Arc reference to failed messages"))?
            .into_inner();
        stats.failed_count = final_failed.len();
        stats.failed_messages = final_msgs;

        // In non-keep-going mode, return the first error after giving
        // independent products a chance to execute and be cached
        if !keep_going && !self.interrupted.load(Ordering::SeqCst) {
            let errs = Arc::try_unwrap(errors)
                .map_err(|_| anyhow::anyhow!("internal error: outstanding Arc reference to errors"))?
                .into_inner();
            if let Some(first_err) = errs.into_iter().next() {
                return Err(first_err);
            }
        }

        Ok(stats)
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
            let product = graph.get_product(id).expect("internal error: invalid product id");

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

            product_level.insert(product.id, my_level);

            // Ensure we have enough levels
            while levels.len() <= my_level {
                levels.push(Vec::new());
            }
            levels[my_level].push(id);
        }

        levels
    }

    /// Clean all products
    pub fn clean(&self, graph: &BuildGraph) -> Result<()> {
        for product in graph.products() {
            if let Some(processor) = self.processors.get(&product.processor) {
                processor.clean(product)?;
            }
        }
        Ok(())
    }
}
