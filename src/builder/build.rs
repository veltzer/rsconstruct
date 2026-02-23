use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant;
use crate::cli::{BuildOptions, BuildPhase, DisplayOptions};
use crate::color;
use crate::errors;
use crate::executor::{Executor, ExecutorOptions};
use super::{Builder, ProductStatusLabels, phases_debug};

impl Builder {
    /// Execute an incremental build using the dependency graph
    pub fn build(&mut self, opts: &BuildOptions, interrupted: Arc<std::sync::atomic::AtomicBool>) -> Result<(), anyhow::Error> {
        // CLI override for spellcheck auto_add_words
        if opts.auto_add_words {
            self.config.processor.spellcheck.auto_add_words = true;
        }

        // CLI override for mtime pre-check
        if opts.no_mtime {
            self.object_store.set_mtime_check(false);
        }

        let processor_filter = opts.processor_filter.as_deref();

        // Create processors
        let t = Instant::now();
        let processors = self.create_processors()?;
        let create_processors_dur = t.elapsed();

        // Validate processor filter against available processors
        if let Some(filter) = processor_filter {
            for name in filter {
                if !processors.contains_key(name) {
                    let available: Vec<_> = processors.keys().collect();
                    return Err(crate::exit_code::RsbError::new(
                        crate::exit_code::RsbExitCode::ConfigError,
                        format!("Unknown processor '{}'. Available: {:?}", name, available),
                    ).into());
                }
            }
        }

        // Check for config changes and display diffs
        self.detect_config_changes(&processors);

        // Build the dependency graph (may stop early based on stop_after)
        let (graph, mut phase_timings) = self.build_graph_with_processors_and_phase(&processors, opts.stop_after, processor_filter, opts.verbose)?;

        // Prepend create_processors timing
        phase_timings.insert(0, ("create_processors".to_string(), create_processors_dur));

        // If we stopped early (before classify), we're done
        if opts.stop_after != BuildPhase::Build && opts.stop_after != BuildPhase::Classify {
            if !crate::runtime_flags::quiet() {
                println!("Stopped after {:?} phase.", opts.stop_after);
            }
            return Ok(());
        }

        // Phase: Classify products (skip/restore/build)
        if phases_debug() {
            eprintln!("{}", color::dim("  Phase: classify"));
        }
        let t = Instant::now();
        let order = graph.topological_sort()?;
        let (skip_count, restore_count, build_count) =
            crate::executor::classify_products(&graph, &order, &self.object_store, opts.force);
        phase_timings.push(("classify".to_string(), t.elapsed()));
        if !crate::runtime_flags::quiet() {
            println!("{} products ({} up-to-date, {} to restore, {} to build)",
                order.len(), skip_count, restore_count, build_count);
        }

        if opts.stop_after == BuildPhase::Classify {
            return Ok(());
        }

        // Create executor with parallelism from command line or config
        let parallel = opts.jobs.unwrap_or(self.config.build.parallel);
        // CLI overrides config for batch_size
        let batch_size = opts.batch_size.unwrap_or(self.config.build.batch_size);
        let executor = Executor::new(&processors, ExecutorOptions {
            parallel,
            verbose: opts.verbose,
            display_opts: opts.display_opts,
            batch_size,
            explain: opts.explain,
            retry: opts.retry,
        }, Arc::clone(&interrupted));

        // Execute the build
        let t = Instant::now();
        let result = executor.execute(&graph, &self.object_store, opts.force, opts.timings, opts.keep_going);
        let build_dur = t.elapsed();

        // Exit if interrupted
        if interrupted.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(crate::exit_code::RsbError::new(
                crate::exit_code::RsbExitCode::Interrupted,
                "Build interrupted",
            ).into());
        }

        let mut stats = result?;

        // Add phase timings to stats
        phase_timings.push(("build".to_string(), build_dur));
        stats.phase_timings = phase_timings;

        // Print summary
        stats.print_summary(opts.summary, opts.timings);

        // Return error if there were failures in keep-going mode
        if stats.failed_count > 0 {
            return Err(crate::exit_code::RsbError::new(
                crate::exit_code::RsbExitCode::BuildError,
                format!("Build completed with {} error(s)", stats.failed_count),
            ).into());
        }

        Ok(())
    }

    /// Show what would happen without executing anything
    pub fn dry_run(&self, force: bool, explain: bool) -> anyhow::Result<()> {
        let processors = self.create_processors()?;
        let graph = self.build_graph_with_processors(&processors)?;

        let order = graph.topological_sort()?;
        if order.is_empty() {
            println!("No products discovered.");
            return Ok(());
        }

        let products: Vec<_> = order.iter()
            .map(|&id| graph.get_product(id).expect(errors::INVALID_PRODUCT_ID))
            .collect();

        let labels = ProductStatusLabels {
            current: (color::dim("SKIP"), "skip"),
            restorable: (color::cyan("RESTORE"), "restore"),
            stale: (color::yellow("BUILD"), "build"),
        };

        self.print_product_status(&products, force, &labels, explain, DisplayOptions::default(), true);
        Ok(())
    }

    /// Show the status of each product in the build graph
    pub fn status(&self, verbose: bool) -> anyhow::Result<()> {
        let processors = self.create_processors()?;
        let graph = self.build_graph_with_processors(&processors)?;

        let products: Vec<&_> = graph.products().iter().collect();
        if products.is_empty() {
            println!("No products discovered.");
            return Ok(());
        }

        let labels = ProductStatusLabels {
            current: (color::green("UP-TO-DATE"), "up-to-date"),
            restorable: (color::cyan("RESTORABLE"), "restorable"),
            stale: (color::yellow("STALE"), "stale"),
        };

        self.print_product_status(&products, false, &labels, false, DisplayOptions::default(), verbose);
        Ok(())
    }

    /// Classify and print the status of each product, with per-processor and total summary.
    /// When `verbose` is false, only the per-processor and total summary lines are printed.
    pub(super) fn print_product_status(
        &self,
        products: &[&crate::graph::Product],
        force: bool,
        labels: &ProductStatusLabels<'_>,
        explain: bool,
        display_opts: DisplayOptions,
        verbose: bool,
    ) {
        use crate::object_store::ExplainAction;

        let mut counts = [0usize; 3]; // [current, restorable, stale]
        let mut per_processor: BTreeMap<&str, [usize; 3]> = BTreeMap::new();

        for product in products {
            let cache_key = product.cache_key();
            let status_idx;
            let input_checksum = match self.object_store.combined_input_checksum_fast(&product.inputs) {
                Ok(cs) => cs,
                Err(_) => {
                    if verbose {
                        println!("{} [{}] {}", labels.stale.0, product.processor, product.display(display_opts));
                    }
                    status_idx = 2;
                    counts[status_idx] += 1;
                    per_processor.entry(&product.processor).or_default()[status_idx] += 1;
                    continue;
                }
            };

            if explain {
                let action = self.object_store.explain_action(&cache_key, &input_checksum, &product.outputs, force);
                let reason_str = format!(" ({})", action);
                status_idx = match action {
                    ExplainAction::Skip => {
                        if verbose {
                            println!("{} [{}] {}{}", labels.current.0, product.processor, product.display(display_opts), reason_str);
                        }
                        0
                    }
                    ExplainAction::Restore(_) => {
                        if verbose {
                            println!("{} [{}] {}{}", labels.restorable.0, product.processor, product.display(display_opts), reason_str);
                        }
                        1
                    }
                    ExplainAction::Rebuild(_) => {
                        if verbose {
                            println!("{} [{}] {}{}", labels.stale.0, product.processor, product.display(display_opts), reason_str);
                        }
                        2
                    }
                };
            } else if !force && !self.object_store.needs_rebuild(&cache_key, &input_checksum, &product.outputs) {
                if verbose {
                    println!("{} [{}] {}", labels.current.0, product.processor, product.display(display_opts));
                }
                status_idx = 0;
            } else if !force && self.object_store.can_restore(&cache_key, &input_checksum, &product.outputs) {
                if verbose {
                    println!("{} [{}] {}", labels.restorable.0, product.processor, product.display(display_opts));
                }
                status_idx = 1;
            } else {
                if verbose {
                    println!("{} [{}] {}", labels.stale.0, product.processor, product.display(display_opts));
                }
                status_idx = 2;
            }

            counts[status_idx] += 1;
            per_processor.entry(&product.processor).or_default()[status_idx] += 1;
        }

        // Per-processor breakdown
        if per_processor.len() > 1 {
            let max_name_len = per_processor.keys().map(|n| n.len()).max().unwrap_or(0);
            for (name, pc) in &per_processor {
                println!("{:width$} {} {}, {} {}, {} {}",
                    format!("{}:", name),
                    pc[0], labels.current.1,
                    pc[1], labels.restorable.1,
                    pc[2], labels.stale.1,
                    width = max_name_len + 1);
            }
        }
        println!("{}: {} {}, {} {}, {} {}",
            color::bold("Summary"),
            counts[0], labels.current.1,
            counts[1], labels.restorable.1,
            counts[2], labels.stale.1);
    }
}
