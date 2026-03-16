use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use anyhow::Result;
use crate::cli::{BuildOptions, BuildPhase, DisplayOptions};
use crate::color;
use crate::errors;
use crate::executor::{Executor, ExecutorOptions};
use crate::processors::{BuildStats, ProcessorMap, ProcessorType};
use super::{Builder, ProductStatusLabels, phases_debug};

/// Expand `@`-prefixed shortcuts in the processor filter.
///
/// Three categories of shortcuts:
/// - **By type**: `@checkers`, `@generators`, `@mass_generators`
/// - **By tool**: `@python3`, `@node`, etc. — matches processors whose `required_tools()` contains the name
/// - **By processor name**: `@ruff` → `"ruff"` — strips the `@` prefix
fn expand_aliases(filter: &[String], processors: &ProcessorMap) -> Vec<String> {
    let mut expanded = Vec::new();
    for name in filter {
        if let Some(alias) = name.strip_prefix('@') {
            match alias {
                "checkers" => {
                    expanded.extend(
                        processors.iter()
                            .filter(|(_, p)| p.processor_type() == ProcessorType::Checker)
                            .map(|(n, _)| n.clone())
                    );
                }
                "generators" => {
                    expanded.extend(
                        processors.iter()
                            .filter(|(_, p)| p.processor_type() == ProcessorType::Generator)
                            .map(|(n, _)| n.clone())
                    );
                }
                "mass_generators" => {
                    expanded.extend(
                        processors.iter()
                            .filter(|(_, p)| p.processor_type() == ProcessorType::MassGenerator)
                            .map(|(n, _)| n.clone())
                    );
                }
                _ => {
                    // Check if it's a tool name
                    let by_tool: Vec<_> = processors.iter()
                        .filter(|(_, p)| p.required_tools().iter().any(|t| t == alias))
                        .map(|(n, _)| n.clone())
                        .collect();
                    if !by_tool.is_empty() {
                        expanded.extend(by_tool);
                    } else if processors.contains_key(alias) {
                        // Fall back to processor name
                        expanded.push(alias.to_string());
                    } else {
                        // Unknown alias — keep original so validation reports the error
                        expanded.push(name.clone());
                    }
                }
            }
        } else {
            expanded.push(name.clone());
        }
    }
    expanded.sort();
    expanded.dedup();
    expanded
}

impl Builder {
    /// Execute an incremental build using the dependency graph
    pub fn build(&mut self, opts: &BuildOptions, interrupted: Arc<std::sync::atomic::AtomicBool>, init_timings: Vec<(String, Duration)>) -> Result<(), anyhow::Error> {
        // CLI override for spellcheck and aspell auto_add_words
        if opts.auto_add_words {
            self.config.processor.spellcheck.auto_add_words = true;
            self.config.processor.aspell.auto_add_words = true;
        }

        // CLI override for mtime pre-check
        if opts.no_mtime {
            self.object_store.set_mtime_check(false);
        }

        // Create processors
        let t = Instant::now();
        let processors = self.create_processors()?;
        let create_processors_dur = t.elapsed();

        // Expand @-prefixed shortcuts in the processor filter
        let expanded_filter = opts.processor_filter.as_ref()
            .map(|f| expand_aliases(f, &processors));
        let processor_filter = expanded_filter.as_deref();

        // Validate processor filter against available processors
        if let Some(filter) = processor_filter {
            for name in filter {
                if !processors.contains_key(name) {
                    let available: Vec<_> = processors.keys().collect();
                    return Err(crate::exit_code::RsconstructError::new(
                        crate::exit_code::RsconstructExitCode::ConfigError,
                        format!("Unknown processor '{}'. Available: {:?}", name, available),
                    ).into());
                }
            }
        }

        // Check for config changes and display diffs
        self.detect_config_changes(&processors);

        // Build the dependency graph (may stop early based on stop_after)
        let (mut graph, mut phase_timings) = self.build_graph_with_processors_and_phase(&processors, opts.stop_after, processor_filter, opts.verbose)?;

        // Filter by target patterns if specified
        if let Some(ref targets) = opts.targets {
            graph.filter_by_targets(targets);
        }

        // Prepend create_processors and init timings
        phase_timings.insert(0, ("create_processors".to_string(), create_processors_dur));
        for (i, timing) in init_timings.into_iter().enumerate() {
            phase_timings.insert(i, timing);
        }

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

        // Execute the build (enable timings collection if trace output is requested)
        let t = Instant::now();
        let collect_timings = opts.timings || opts.trace.is_some();
        let result = executor.execute(&graph, &self.object_store, opts.force, collect_timings, opts.keep_going);
        let build_dur = t.elapsed();

        // Exit if interrupted
        if interrupted.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(crate::exit_code::RsconstructError::new(
                crate::exit_code::RsconstructExitCode::Interrupted,
                "Build interrupted",
            ).into());
        }

        let mut stats = result?;

        // Add phase timings to stats
        phase_timings.push(("build".to_string(), build_dur));
        stats.phase_timings = phase_timings;

        // Print summary
        stats.print_summary(opts.summary, opts.timings);

        // Write Chrome trace file if requested
        if let Some(ref trace_path) = opts.trace {
            write_trace_file(trace_path, &stats)?;
        }

        // Return error if there were failures in keep-going mode
        if stats.failed_count > 0 {
            return Err(crate::exit_code::RsconstructError::new(
                crate::exit_code::RsconstructExitCode::BuildError,
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

/// Write a Chrome trace format JSON file from build statistics.
/// The file can be opened in chrome://tracing or https://ui.perfetto.dev
fn write_trace_file(path: &str, stats: &BuildStats) -> Result<()> {
    let mut events: Vec<serde_json::Value> = Vec::new();
    let mut tid_counter = 1u64;

    // Phase timings on tid=0
    let mut phase_offset_us = 0i64;
    for (name, dur) in &stats.phase_timings {
        let dur_us = dur.as_micros() as i64;
        events.push(serde_json::json!({
            "name": name,
            "cat": "phase",
            "ph": "X",
            "ts": phase_offset_us,
            "dur": dur_us,
            "pid": 1,
            "tid": 0
        }));
        phase_offset_us += dur_us;
    }

    // Product timings
    for cat in &stats.categories {
        for pt in &cat.product_timings {
            let dur_us = pt.duration.as_micros() as i64;
            let ts_us = pt.start_offset
                .map(|off| off.as_micros() as i64)
                .unwrap_or(0);
            let name = format!("{}:{}", pt.processor, pt.display);
            events.push(serde_json::json!({
                "name": name,
                "cat": "build",
                "ph": "X",
                "ts": ts_us,
                "dur": dur_us,
                "pid": 1,
                "tid": tid_counter
            }));
            tid_counter += 1;
        }
    }

    let trace = serde_json::json!({ "traceEvents": events });
    std::fs::write(path, serde_json::to_string_pretty(&trace)?)?;
    if !crate::runtime_flags::quiet() {
        println!("Wrote trace to {}", color::bold(path));
    }
    Ok(())
}
