use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use indicatif::ProgressBar;
use parking_lot::Mutex;
use std::thread;
use std::time::Instant;

use crate::color;
use crate::errors;
use crate::graph::BuildGraph;
use crate::json_output;
use crate::object_store::ObjectStore;
use crate::processors::{BuildStats, ProductTiming};
use crate::progress;

use super::{Executor, HandlerContext, LevelWork, PreCheckResult, RestoreOutcome, SharedState, WorkItem};

/// Per-processor progress counters shared across non-batch threads.
struct ProgressCounters {
    total_per_processor: Arc<HashMap<String, usize>>,
    current_per_processor: Arc<Mutex<HashMap<String, usize>>>,
}

/// Common context shared by both batch and non-batch processing threads within a level.
struct LevelContext<'b> {
    graph: &'b BuildGraph,
    object_store: &'b ObjectStore,
    force: bool,
    keep_going: bool,
    timings: bool,
    shared: &'b SharedState,
    pb: &'b ProgressBar,
}

impl<'a> Executor<'a> {
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
            let product = graph.get_product(product_id).expect(errors::INVALID_PRODUCT_ID);
            *total_per_processor.entry(product.processor.clone()).or_insert(0) += 1;
        }
        let counters = ProgressCounters {
            total_per_processor: Arc::new(total_per_processor),
            current_per_processor: Arc::new(Mutex::new(HashMap::new())),
        };
        let global_total = order.len();

        // Pre-build classification: count skip/restore/build for progress bar sizing
        let (_skip_count, restore_count, build_count) = Self::classify_products(graph, order, object_store, force);
        let work_count = restore_count + build_count;

        // Create progress bar sized to actual work (excludes instant skips)
        let pb = progress::create_bar(
            work_count as u64,
            self.verbose || json_output::is_json_mode(),
        );

        let shared = SharedState {
            stats: Arc::new(Mutex::new(HashMap::new())),
            errors: Arc::new(Mutex::new(Vec::new())),
            failed_products: Arc::new(Mutex::new(HashSet::new())),
            failed_messages: Arc::new(Mutex::new(Vec::new())),
            failed_processors: Arc::new(Mutex::new(HashSet::new())),
            unchanged_products: Arc::new(Mutex::new(HashSet::new())),
            global_current: Arc::new(AtomicUsize::new(0)),
            global_total,
        };

        for level in levels {
            // Check for Ctrl+C before starting next level
            if self.is_interrupted() {
                break;
            }

            let LevelWork { batch_groups, non_batch_items } = self.prepare_level_work(
                graph, &level, object_store, force, keep_going, &shared,
            );

            let lctx = LevelContext {
                graph, object_store, force, keep_going, timings,
                shared: &shared, pb: &pb,
            };

            // Process this level in parallel using thread pool
            thread::scope(|s| {
                let lctx_ref = &lctx;

                // Spawn one thread per batch group
                for (proc_name, items) in &batch_groups {
                    s.spawn(move || {
                        self.process_batch_group(proc_name, items, lctx_ref);
                    });
                }

                // Spawn threads for non-batch items (chunked across threads)
                if !non_batch_items.is_empty() {
                    let chunk_size = non_batch_items.len().div_ceil(self.parallel);

                    for chunk in non_batch_items.chunks(chunk_size.max(1)) {
                        let total_ref = Arc::clone(&counters.total_per_processor);
                        let current_ref = Arc::clone(&counters.current_per_processor);

                        s.spawn(move || {
                            self.process_non_batch_chunk(
                                chunk, lctx_ref, &total_ref, &current_ref,
                            );
                        });
                    }
                }
            });

            // If interrupted, stop processing further levels
            if self.is_interrupted() {
                println!("{}", color::yellow("Interrupted, saving progress..."));
                break;
            }

            // In non-keep-going mode, stop after level with errors
            if !keep_going && !shared.errors.lock().is_empty() {
                break;
            }
        }

        pb.finish_and_clear();
        Self::collect_build_stats(shared, keep_going, self.is_interrupted())
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
        let (id, input_checksum, needs_rebuild) = item;
        let product = lctx.graph.get_product(*id).expect(errors::INVALID_PRODUCT_ID);
        let cache_key = product.cache_key();

        if self.explain {
            let action = lctx.object_store.explain_action(&cache_key, input_checksum, &product.outputs, lctx.force);
            self.print_explain(product, &action);
        }

        if !needs_rebuild {
            self.handle_skip(product, lctx.shared);
            return PreCheckResult::Handled;
        }

        let ctx = HandlerContext {
            product, id: *id, cache_key, input_checksum,
            proc_name, keep_going: lctx.keep_going, shared: lctx.shared, pb: lctx.pb,
        };
        match self.handle_restore(&ctx, lctx.object_store, lctx.force, emit_fail_event) {
            RestoreOutcome::Restored | RestoreOutcome::Failed => PreCheckResult::Handled,
            RestoreOutcome::NotRestorable => PreCheckResult::NeedsExecution,
        }
    }

    /// Process a single batch group within a thread.
    fn process_batch_group(
        &self,
        proc_name: &str,
        items: &[WorkItem],
        lctx: &LevelContext,
    ) {
        if self.is_interrupted() {
            return;
        }

        let processor = match self.processors.get(proc_name) {
            Some(p) => p,
            None => return,
        };

        // Handle skip/restore for items that don't need rebuild
        let mut to_execute: Vec<&WorkItem> = Vec::new();
        for item in items {
            match self.try_skip_or_restore(item, proc_name, lctx, false) {
                PreCheckResult::Handled => continue,
                PreCheckResult::NeedsExecution => to_execute.push(item),
            }
        }

        if to_execute.is_empty() || self.is_interrupted() {
            return;
        }

        let proc_total = items.len();
        let mut proc_current = items.len() - to_execute.len();

        // Determine chunk size: 0 means no limit
        let chunk_size = match self.batch_size {
            Some(0) | None => to_execute.len(),
            Some(n) => n,
        };

        // Process in chunks
        for chunk in to_execute.chunks(chunk_size) {
            if self.is_interrupted() {
                break;
            }

            // Execute batch chunk
            let product_refs: Vec<&crate::graph::Product> = chunk.iter()
                .map(|(id, _, _)| lctx.graph.get_product(*id).expect(errors::INVALID_PRODUCT_ID))
                .collect();

            proc_current += chunk.len();
            if self.verbose {
                let display = product_refs.iter()
                    .map(|p| self.product_display(p))
                    .collect::<Vec<_>>()
                    .join(", ");
                let gc = lctx.shared.global_current.load(Ordering::SeqCst);
                println!("[{}] ({}/{}) ({}/{}) {} {} files: {}",
                    proc_name,
                    gc + 1, lctx.shared.global_total,
                    proc_current, proc_total,
                    color::green("Processing batch:"),
                    product_refs.len(),
                    display);
            } else {
                lctx.pb.set_message(format!("[{}] batch {} files", proc_name, product_refs.len()));
            }

            for p in &product_refs {
                json_output::emit_product_start(&self.product_display(p), &p.processor);
            }
            let batch_start = Instant::now();
            let results = processor.execute_batch(&product_refs);
            let batch_duration = batch_start.elapsed();

            // Process per-product results
            for (item, result) in chunk.iter().zip(results) {
                let (id, input_checksum, _) = item;
                let product = lctx.graph.get_product(*id).expect(errors::INVALID_PRODUCT_ID);
                let cache_key = product.cache_key();

                let ctx = HandlerContext {
                    product, id: *id, cache_key, input_checksum,
                    proc_name, keep_going: lctx.keep_going, shared: lctx.shared, pb: lctx.pb,
                };
                match result {
                    Ok(()) => {
                        if !self.handle_success(&ctx, lctx.object_store, None) {
                            // cache_outputs failed and error was handled
                            continue;
                        }
                    }
                    Err(e) => {
                        self.handle_error(&ctx, e, None);
                    }
                }
                Self::inc_progress(lctx.pb, lctx.shared);
            }

            // Record batch timing for this chunk
            if lctx.timings {
                let mut stats = lctx.shared.stats.lock();
                let proc_stats = stats
                    .entry(proc_name.to_string())
                    .or_default();
                proc_stats.duration += batch_duration;
                proc_stats.product_timings.push(ProductTiming {
                    display: format!("batch ({} files)", product_refs.len()),
                    processor: proc_name.to_string(),
                    duration: batch_duration,
                });
            }
        }
    }

    /// Process a chunk of non-batch work items within a thread.
    fn process_non_batch_chunk(
        &self,
        chunk: &[WorkItem],
        lctx: &LevelContext,
        total_per_processor: &HashMap<String, usize>,
        current_per_processor: &Mutex<HashMap<String, usize>>,
    ) {
        for item @ (id, _input_checksum, _needs_rebuild) in chunk {
            // Stop if interrupted or if there's an error (non-keep-going mode)
            if self.is_interrupted()
                || (!lctx.keep_going && !lctx.shared.errors.lock().is_empty())
            {
                break;
            }

            let product = lctx.graph.get_product(*id).expect(errors::INVALID_PRODUCT_ID);

            if let PreCheckResult::Handled = self.try_skip_or_restore(item, &product.processor, lctx, true) {
                continue;
            }

            if let Some(processor) = self.processors.get(&product.processor) {
                let cache_key = product.cache_key();
                let input_checksum = &item.1;
                let ctx = HandlerContext {
                    product, id: *id, cache_key, input_checksum,
                    proc_name: &product.processor, keep_going: lctx.keep_going,
                    shared: lctx.shared, pb: lctx.pb,
                };

                // Update progress counter
                let current = {
                    let mut current_guard = current_per_processor.lock();
                    let c = current_guard.entry(product.processor.clone()).or_insert(0);
                    *c += 1;
                    *c
                };
                let total = total_per_processor.get(&product.processor).copied()
                    .expect(errors::PROCESSOR_NOT_IN_TOTALS);

                if self.verbose {
                    let variant_tag = product.variant.as_ref()
                        .map(|v| format!(":{}", v))
                        .unwrap_or_default();
                    let gc = lctx.shared.global_current.load(Ordering::SeqCst) + 1;
                    println!("[{}{}] ({}/{}) ({}/{}) {} {}", product.processor, variant_tag,
                        gc, lctx.shared.global_total,
                        current, total,
                        color::green("Processing:"),
                        self.product_display(product));
                } else {
                    let variant_tag = product.variant.as_ref()
                        .map(|v| format!(":{}", v))
                        .unwrap_or_default();
                    lctx.pb.set_message(format!("[{}{}] {}", product.processor, variant_tag, self.product_display(product)));
                }

                json_output::emit_product_start(&self.product_display(product), &product.processor);
                let product_start = Instant::now();
                match processor.execute(product) {
                    Ok(()) => {
                        let duration = product_start.elapsed();

                        if !self.handle_success(&ctx, lctx.object_store, Some(duration)) {
                            // cache_outputs failed and error was handled
                            continue;
                        }

                        // Record per-product duration (non-batch only)
                        {
                            let mut stats = lctx.shared.stats.lock();
                            let proc_stats = stats
                                .entry(product.processor.clone())
                                .or_default();
                            proc_stats.duration += duration;
                            if lctx.timings {
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
                        self.handle_error(&ctx, e, Some(duration));
                    }
                }
                Self::inc_progress(lctx.pb, lctx.shared);
            }
        }
    }

    /// Collect final build stats from shared state after all levels complete.
    fn collect_build_stats(shared: SharedState, keep_going: bool, interrupted: bool) -> Result<BuildStats> {
        let final_stats = Arc::try_unwrap(shared.stats)
            .map_err(|_| anyhow::anyhow!("internal error: outstanding Arc reference to stats"))?
            .into_inner();
        let mut stats = BuildStats::default();
        for (_, proc_stats) in final_stats {
            stats.add(proc_stats);
        }

        let final_failed = Arc::try_unwrap(shared.failed_products)
            .map_err(|_| anyhow::anyhow!("internal error: outstanding Arc reference to failed products"))?
            .into_inner();
        let final_msgs = Arc::try_unwrap(shared.failed_messages)
            .map_err(|_| anyhow::anyhow!("internal error: outstanding Arc reference to failed messages"))?
            .into_inner();
        stats.failed_count = final_failed.len();
        stats.failed_messages = final_msgs;

        // In non-keep-going mode, return the first error after giving
        // independent products a chance to execute and be cached
        if !keep_going && !interrupted {
            let errs = Arc::try_unwrap(shared.errors)
                .map_err(|_| anyhow::anyhow!("internal error: outstanding Arc reference to errors"))?
                .into_inner();
            if let Some(first_err) = errs.into_iter().next() {
                return Err(first_err);
            }
        }

        Ok(stats)
    }

    /// Prepare work items for a single parallel level.
    ///
    /// Skips products with failed dependencies, computes checksums,
    /// and separates items into batch groups vs non-batch items.
    pub(super) fn prepare_level_work(
        &self,
        graph: &BuildGraph,
        level: &[usize],
        object_store: &ObjectStore,
        force: bool,
        keep_going: bool,
        shared: &SharedState,
    ) -> LevelWork {
        // Each work item: (product_id, input_checksum, needs_rebuild)
        let mut work_items: Vec<WorkItem> = Vec::new();

        // First pass: identify products with failed dependencies
        let mut skipped_ids: HashSet<usize> = HashSet::new();
        {
            let failed_guard = shared.failed_products.lock();
            for &id in level {
                if self.has_failed_dependency(graph, id, &failed_guard) {
                    let product = graph.get_product(id).expect(errors::INVALID_PRODUCT_ID);
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
                let cache_key = product.cache_key();

                // Early cutoff: if all dependencies produced identical outputs,
                // reuse the cached input checksum instead of recomputing
                let deps = graph.get_dependencies(id);
                let input_checksum = {
                    let unchanged_guard = shared.unchanged_products.lock();
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
                            shared.failed_products.lock().insert(id);
                            shared.failed_messages.lock().push(msg);
                        } else {
                            shared.failed_products.lock().insert(id);
                            shared.errors.lock().push(e);
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
        let mut batch_groups: HashMap<String, Vec<WorkItem>> = HashMap::new();
        let mut non_batch_items: Vec<WorkItem> = Vec::new();

        // Group all items by processor name
        let mut by_processor: HashMap<String, Vec<WorkItem>> = HashMap::new();
        for item in work_items {
            let product = graph.get_product(item.0).expect(errors::INVALID_PRODUCT_ID);
            by_processor.entry(product.processor.clone()).or_default().push(item);
        }

        // Separate into batch vs non-batch
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

        LevelWork { batch_groups, non_batch_items }
    }
}
