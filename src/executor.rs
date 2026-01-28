use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

use crate::color;
use crate::graph::BuildGraph;
use crate::object_store::ObjectStore;
use crate::processors::{BuildStats, ProcessStats, ProductDiscovery, ProductTiming};

/// Executor handles running products through their processors
/// It respects dependency order and can parallelize independent products
pub struct Executor<'a> {
    processors: &'a HashMap<String, Box<dyn ProductDiscovery>>,
    parallel: usize,
}

impl<'a> Executor<'a> {
    pub fn new(processors: &'a HashMap<String, Box<dyn ProductDiscovery>>, parallel: usize) -> Self {
        Self {
            processors,
            parallel,
        }
    }

    /// Execute all products in the graph that need rebuilding
    pub fn execute(
        &self,
        graph: &BuildGraph,
        object_store: &mut ObjectStore,
        force: bool,
        verbose: bool,
        timings: bool,
        keep_going: bool,
    ) -> Result<BuildStats> {
        let build_start = Instant::now();
        let order = graph.topological_sort()?;

        let result = if self.parallel <= 1 {
            self.execute_sequential(graph, &order, object_store, force, verbose, timings, keep_going)
        } else {
            self.execute_parallel(graph, &order, object_store, force, verbose, timings, keep_going)
        };

        match result {
            Ok(mut stats) => {
                stats.total_duration = build_start.elapsed();
                Ok(stats)
            }
            Err(e) => Err(e),
        }
    }

    /// Execute products sequentially
    fn execute_sequential(
        &self,
        graph: &BuildGraph,
        order: &[usize],
        object_store: &mut ObjectStore,
        force: bool,
        verbose: bool,
        timings: bool,
        keep_going: bool,
    ) -> Result<BuildStats> {
        let mut stats_by_processor: HashMap<String, ProcessStats> = HashMap::new();
        let mut failed_products: HashSet<usize> = HashSet::new();
        let mut failed_messages: Vec<String> = Vec::new();

        for &id in order {
            let product = graph.get_product(id).unwrap();

            // If keep_going, skip products whose dependencies have failed
            if keep_going && self.has_failed_dependency(graph, id, &failed_products) {
                if verbose {
                    println!("[{}] {} {}", product.processor,
                        color::yellow("Skipping (dependency failed):"),
                        product.display());
                }
                failed_products.insert(id);
                continue;
            }

            let cache_key = product.cache_key();
            let input_checksum = ObjectStore::combined_input_checksum(&product.inputs)?;

            // Check if this product needs rebuilding
            if !force && !object_store.needs_rebuild(&cache_key, &input_checksum, &product.outputs) {
                if verbose {
                    println!("[{}] {} {}", product.processor,
                        color::dim("Skipping (unchanged):"),
                        product.display());
                }
                let stats = stats_by_processor
                    .entry(product.processor.clone())
                    .or_insert_with(|| ProcessStats::new(&product.processor));
                stats.skipped += 1;
                continue;
            }

            // Try to restore from cache if outputs are missing
            if !force && object_store.restore_from_cache(&cache_key, &input_checksum, &product.outputs)? {
                if verbose {
                    println!("[{}] {} {}", product.processor,
                        color::cyan("Restored from cache:"),
                        product.display());
                }
                let stats = stats_by_processor
                    .entry(product.processor.clone())
                    .or_insert_with(|| ProcessStats::new(&product.processor));
                stats.skipped += 1;
                continue;
            }

            // Find the processor and execute
            if let Some(processor) = self.processors.get(&product.processor) {
                println!("[{}] {} {}", product.processor,
                    color::green("Processing:"),
                    product.display());

                let product_start = Instant::now();
                match processor.execute(product) {
                    Ok(()) => {
                        let duration = product_start.elapsed();

                        // Cache outputs
                        object_store.cache_outputs(&cache_key, &input_checksum, &product.outputs)?;

                        let stats = stats_by_processor
                            .entry(product.processor.clone())
                            .or_insert_with(|| ProcessStats::new(&product.processor));
                        stats.processed += 1;
                        stats.duration += duration;
                        if timings {
                            stats.product_timings.push(ProductTiming {
                                display: product.display(),
                                processor: product.processor.clone(),
                                duration,
                            });
                        }
                    }
                    Err(e) => {
                        if keep_going {
                            let msg = format!("[{}] {}: {}", product.processor, product.display(), e);
                            println!("{}", color::red(&format!("Error: {}", msg)));
                            failed_products.insert(id);
                            failed_messages.push(msg);
                        } else {
                            return Err(e);
                        }
                    }
                }
            }
        }

        // Build aggregated stats
        let mut stats = BuildStats::default();
        for (_, proc_stats) in stats_by_processor {
            stats.add(proc_stats);
        }
        stats.failed_count = failed_products.len();
        stats.failed_messages = failed_messages;

        Ok(stats)
    }

    /// Execute products in parallel where dependencies allow
    fn execute_parallel(
        &self,
        graph: &BuildGraph,
        order: &[usize],
        object_store: &mut ObjectStore,
        force: bool,
        verbose: bool,
        timings: bool,
        keep_going: bool,
    ) -> Result<BuildStats> {
        // Group products into levels that can run in parallel
        let levels = self.compute_parallel_levels(graph, order);

        let stats_by_processor: Arc<Mutex<HashMap<String, ProcessStats>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let store = Arc::new(Mutex::new(std::mem::take(object_store)));
        let errors: Arc<Mutex<Vec<anyhow::Error>>> = Arc::new(Mutex::new(Vec::new()));
        let failed_products: Arc<Mutex<HashSet<usize>>> = Arc::new(Mutex::new(HashSet::new()));
        let failed_messages: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

        for level in levels {
            // Determine which products in this level need work
            let mut work_items: Vec<(usize, String, bool)> = Vec::new();

            // First pass: identify products with failed dependencies
            let mut skipped_ids: Vec<usize> = Vec::new();
            if keep_going {
                let failed_guard = failed_products.lock().unwrap();
                for &id in &level {
                    if self.has_failed_dependency(graph, id, &failed_guard) {
                        let product = graph.get_product(id).unwrap();
                        if verbose {
                            println!("[{}] {} {}", product.processor,
                                color::yellow("Skipping (dependency failed):"),
                                product.display());
                        }
                        skipped_ids.push(id);
                    }
                }
            }
            if !skipped_ids.is_empty() {
                let mut failed_guard = failed_products.lock().unwrap();
                for id in &skipped_ids {
                    failed_guard.insert(*id);
                }
            }

            // Second pass: determine work items for non-skipped products
            {
                let store_guard = store.lock().unwrap();
                for &id in &level {
                    if skipped_ids.contains(&id) {
                        continue;
                    }

                    let product = graph.get_product(id).unwrap();
                    let cache_key = product.cache_key();
                    let input_checksum = match ObjectStore::combined_input_checksum(&product.inputs) {
                        Ok(cs) => cs,
                        Err(e) => {
                            if keep_going {
                                let msg = format!("[{}] {}: {}", product.processor, product.display(), e);
                                println!("{}", color::red(&format!("Error: {}", msg)));
                                failed_products.lock().unwrap().insert(id);
                                failed_messages.lock().unwrap().push(msg);
                            } else {
                                errors.lock().unwrap().push(e);
                            }
                            continue;
                        }
                    };

                    let needs = force || store_guard.needs_rebuild(&cache_key, &input_checksum, &product.outputs);
                    work_items.push((id, input_checksum, needs));
                }
            }

            // Process this level in parallel using thread pool
            let chunk_size = (work_items.len() + self.parallel - 1) / self.parallel;
            let chunks: Vec<_> = work_items.chunks(chunk_size.max(1)).collect();

            thread::scope(|s| {
                for chunk in chunks {
                    let stats_ref = Arc::clone(&stats_by_processor);
                    let store_ref = Arc::clone(&store);
                    let errors_ref = Arc::clone(&errors);
                    let failed_ref = Arc::clone(&failed_products);
                    let failed_msgs_ref = Arc::clone(&failed_messages);

                    s.spawn(move || {
                        for (id, input_checksum, needs_rebuild) in chunk {
                            let product = graph.get_product(*id).unwrap();
                            let cache_key = product.cache_key();

                            if !needs_rebuild {
                                if verbose {
                                    println!("[{}] {} {}", product.processor,
                                        color::dim("Skipping (unchanged):"),
                                        product.display());
                                }
                                let mut stats = stats_ref.lock().unwrap();
                                let proc_stats = stats
                                    .entry(product.processor.clone())
                                    .or_insert_with(|| ProcessStats::new(&product.processor));
                                proc_stats.skipped += 1;
                                continue;
                            }

                            // Try to restore from cache
                            if !force {
                                let restore_result = {
                                    let store_guard = store_ref.lock().unwrap();
                                    store_guard.restore_from_cache(&cache_key, input_checksum, &product.outputs)
                                };
                                match restore_result {
                                    Ok(true) => {
                                        if verbose {
                                            println!("[{}] {} {}", product.processor,
                                                color::cyan("Restored from cache:"),
                                                product.display());
                                        }
                                        let mut stats = stats_ref.lock().unwrap();
                                        let proc_stats = stats
                                            .entry(product.processor.clone())
                                            .or_insert_with(|| ProcessStats::new(&product.processor));
                                        proc_stats.skipped += 1;
                                        continue;
                                    }
                                    Err(e) => {
                                        if keep_going {
                                            let msg = format!("[{}] {}: {}", product.processor, product.display(), e);
                                            println!("{}", color::red(&format!("Error: {}", msg)));
                                            failed_ref.lock().unwrap().insert(*id);
                                            failed_msgs_ref.lock().unwrap().push(msg);
                                        } else {
                                            errors_ref.lock().unwrap().push(e);
                                        }
                                        continue;
                                    }
                                    Ok(false) => {}
                                }
                            }

                            if let Some(processor) = self.processors.get(&product.processor) {
                                println!("[{}] {} {}", product.processor,
                                    color::green("Processing:"),
                                    product.display());

                                let product_start = Instant::now();
                                match processor.execute(product) {
                                    Ok(()) => {
                                        let duration = product_start.elapsed();

                                        // Cache outputs
                                        {
                                            let mut store_guard = store_ref.lock().unwrap();
                                            if let Err(e) = store_guard.cache_outputs(&cache_key, input_checksum, &product.outputs) {
                                                if keep_going {
                                                    let msg = format!("[{}] {}: {}", product.processor, product.display(), e);
                                                    println!("{}", color::red(&format!("Error: {}", msg)));
                                                    failed_ref.lock().unwrap().insert(*id);
                                                    failed_msgs_ref.lock().unwrap().push(msg);
                                                } else {
                                                    errors_ref.lock().unwrap().push(e);
                                                }
                                                continue;
                                            }
                                        }

                                        let mut stats = stats_ref.lock().unwrap();
                                        let proc_stats = stats
                                            .entry(product.processor.clone())
                                            .or_insert_with(|| ProcessStats::new(&product.processor));
                                        proc_stats.processed += 1;
                                        proc_stats.duration += duration;
                                        if timings {
                                            proc_stats.product_timings.push(ProductTiming {
                                                display: product.display(),
                                                processor: product.processor.clone(),
                                                duration,
                                            });
                                        }
                                    }
                                    Err(e) => {
                                        if keep_going {
                                            let msg = format!("[{}] {}: {}", product.processor, product.display(), e);
                                            println!("{}", color::red(&format!("Error: {}", msg)));
                                            failed_ref.lock().unwrap().insert(*id);
                                            failed_msgs_ref.lock().unwrap().push(msg);
                                        } else {
                                            errors_ref.lock().unwrap().push(e);
                                        }
                                        continue;
                                    }
                                }
                            }
                        }
                    });
                }
            });

            // Check for errors after each level (only in non-keep-going mode)
            if !keep_going {
                let errs = errors.lock().unwrap();
                if !errs.is_empty() {
                    // Restore store before returning error
                    *object_store = Arc::try_unwrap(store).unwrap().into_inner().unwrap();
                    return Err(anyhow::anyhow!("Build failed: {}", errs[0]));
                }
            }
        }

        // Restore the store
        *object_store = Arc::try_unwrap(store).unwrap().into_inner().unwrap();

        // Build aggregated stats
        let final_stats = Arc::try_unwrap(stats_by_processor).unwrap().into_inner().unwrap();
        let mut stats = BuildStats::default();
        for (_, proc_stats) in final_stats {
            stats.add(proc_stats);
        }

        let final_failed = Arc::try_unwrap(failed_products).unwrap().into_inner().unwrap();
        let final_msgs = Arc::try_unwrap(failed_messages).unwrap().into_inner().unwrap();
        stats.failed_count = final_failed.len();
        stats.failed_messages = final_msgs;

        // In non-keep-going mode, check remaining errors
        if !keep_going {
            let errs = Arc::try_unwrap(errors).unwrap().into_inner().unwrap();
            if !errs.is_empty() {
                return Err(anyhow::anyhow!("Build failed: {}", errs[0]));
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
            let product = graph.get_product(id).unwrap();

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
