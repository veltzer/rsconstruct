use super::{
    Builder, GraphSnapshot, ProductStatusLabels, StatusPrintOptions, phases_debug,
    print_graph_stats,
};
use crate::cli::{BuildOptions, BuildPhase, DisplayOptions};
use crate::color;
use crate::errors;
use crate::executor::{BuildPolicy as _, Executor, ExecutorOptions};
use crate::object_store::{ExplainAction, RebuildReason};
use crate::processor::{ProcessorMap, ProcessorType};
use crate::stats::BuildStats;
use crate::tables;
use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::fmt::Write;
use std::time::{Duration, Instant};

/// Expand `@`-prefixed shortcuts in the processor filter.
///
/// - **By type**: `@checker`, `@generator`, `@creator`, `@explicit`,
///   `@mass_generator`, `@lua` — every processor (and instance) of that type.
///   A type with no configured processors selects nothing.
/// - **Anything else**, `@x`, selects the union of the processors whose
///   `required_tools()` contains `x` (`@python3`) and the processors whose
///   short name is `x` (`@tera` → `processor.generator.tera` and each of its
///   instances; `@generic` → every generic processor of every type).
/// - An `@x` that selects nothing stays as written, so validation reports
///   it as an unknown processor.
///
/// The short-name case used to compare the bare word against full names
/// (`processor.generator.tera`), which never matched: `@tera` failed, and
/// `@ruff` only worked because `ruff` is also a tool name.
fn expand_aliases(filter: &[String], processors: &ProcessorMap) -> Vec<String> {
    use crate::registries::processor::parse_name;
    let mut expanded = Vec::new();
    for name in filter {
        let Some(alias) = name.strip_prefix('@') else {
            expanded.push(name.clone());
            continue;
        };
        if let Some(processor_type) = ProcessorType::parse(alias) {
            expanded.extend(
                processors
                    .keys()
                    .filter(|n| parse_name(n).is_some_and(|p| p.processor_type == processor_type))
                    .cloned(),
            );
            continue;
        }
        let selected: Vec<String> = processors
            .iter()
            .filter(|(n, p)| {
                p.required_tools().iter().any(|t| t == alias)
                    || parse_name(n).is_some_and(|parsed| parsed.short == alias)
            })
            .map(|(n, _)| n.clone())
            .collect();
        if selected.is_empty() {
            expanded.push(name.clone());
        } else {
            expanded.extend(selected);
        }
    }
    expanded.sort();
    expanded.dedup();
    expanded
}

/// Verify that every enabled processor's required tools exist on PATH.
/// `only` restricts the check to the named processor instances (used by the
/// deferred, product-aware check in shared-config mode); `None` checks every
/// enabled instance that passes `processor_filter`.
fn check_required_tools(
    processors: &ProcessorMap,
    processor_filter: Option<&[String]>,
    only: Option<&std::collections::HashSet<&str>>,
) -> Result<()> {
    let active_names: Vec<&String> = processors
        .keys()
        .filter(|k| processor_filter.is_none_or(|filter| filter.iter().any(|f| f == *k)))
        .filter(|k| only.is_none_or(|set| set.contains(k.as_str())))
        .filter(|k| processors[*k].scan_config().enabled)
        .collect();
    let mut missing: Vec<(String, Vec<String>)> = Vec::new();
    let mut checked: std::collections::HashSet<String> = std::collections::HashSet::new();
    for name in &active_names {
        for tool in processors[*name].required_tools() {
            if !checked.insert(tool.clone()) {
                continue;
            }
            if which::which(&tool).is_err() {
                let procs: Vec<String> = active_names
                    .iter()
                    .filter(|n| processors[**n].required_tools().contains(&tool))
                    .map(|n| (*n).clone())
                    .collect();
                missing.push((tool, procs));
            }
        }
    }
    if !missing.is_empty() {
        missing.sort_by(|a, b| a.0.cmp(&b.0));
        let mut msg = String::from("Missing required tools:\n");
        for (tool, procs) in &missing {
            let install_hint = crate::tools::tool_install_command(tool)
                .map(|cmd| format!("  install: {cmd}"))
                .unwrap_or_default();
            let _ = writeln!(
                msg,
                "  {} (needed by: {}){}",
                tool,
                procs.join(", "),
                install_hint
            );
        }
        msg.push_str("\nRun `rsconstruct tool install` to install missing tools.");
        return Err(crate::exit_code::RsconstructError::new(
            crate::exit_code::RsconstructExitCode::ToolError,
            msg.trim_end(),
        )
        .into());
    }
    Ok(())
}

/// Resolve `-p`/`-x` into the single allow-list the rest of the build uses.
///
/// Pure CLI semantics — alias expansion, unknown-name and conflict reporting,
/// and the `-x`-only case where an include list is synthesized from
/// "everything minus the excludes". Extracted from `Builder::build`, which
/// mixed this argument-massaging in with orchestration; as a free function it
/// is directly testable without standing up a `Builder`.
///
/// Returns `None` when neither filter is given, meaning "run everything".
fn resolve_processor_filter(
    include: Option<&[String]>,
    exclude: Option<&[String]>,
    processors: &ProcessorMap,
) -> Result<Option<Vec<String>>, anyhow::Error> {
    let include_expanded = include.map(|f| expand_aliases(f, processors));
    let exclude_expanded = exclude.map(|f| expand_aliases(f, processors));

    // Validate unknown names in either filter, pooling both error reports.
    let mut unknown: Vec<String> = Vec::new();
    for filter in [&include_expanded, &exclude_expanded]
        .iter()
        .copied()
        .flatten()
    {
        for name in filter {
            if !processors.contains_key(name) {
                unknown.push(name.clone());
            }
        }
    }
    if !unknown.is_empty() {
        let mut available: Vec<&String> = processors.keys().collect();
        available.sort();
        return Err(crate::exit_code::RsconstructError::new(
            crate::exit_code::RsconstructExitCode::ConfigError,
            format!("Unknown processor(s): {unknown:?}. Available: {available:?}"),
        )
        .into());
    }

    // Reject overlap between -p and -x: contradictory intent should fail
    // loudly rather than silently picking one side.
    if let (Some(inc), Some(exc)) = (&include_expanded, &exclude_expanded) {
        let conflicts: Vec<&String> = exc.iter().filter(|e| inc.contains(e)).collect();
        if !conflicts.is_empty() {
            return Err(crate::exit_code::RsconstructError::new(
                crate::exit_code::RsconstructExitCode::ConfigError,
                format!("Processor(s) {conflicts:?} appear in both -p and -x"),
            )
            .into());
        }
    }

    // When -x is the only filter, synthesize an include list from "all
    // processors minus excludes" so downstream code can treat it as a
    // regular allow-list.
    Ok(match (include_expanded, exclude_expanded) {
        (Some(inc), Some(exc)) => Some(inc.into_iter().filter(|n| !exc.contains(n)).collect()),
        (Some(inc), None) => Some(inc),
        (None, Some(exc)) => Some(
            processors
                .keys()
                .filter(|n| !exc.contains(n))
                .cloned()
                .collect(),
        ),
        (None, None) => None,
    })
}

/// Apply `-p`/`-x` to a fully discovered graph.
///
/// `allowed` is the resolved allow-list (see `resolve_processor_filter`).
/// Its processors' products are kept together with every product that
/// generates their inputs, transitively — the processors that make a
/// selected processor's generated inputs run too, as `--target` already
/// does for its products. (`-p` used to restrict discovery instead, so
/// those producers never ran and, on a clean checkout, the selected
/// processor found none of its generated inputs.) An explicitly
/// `excluded` processor is never pulled back in, and products that need
/// its outputs are dropped with it: they could only run against files that
/// are not being produced.
///
/// Before dependencies are resolved (`--stop-after discover` or
/// `add-dependencies`) there are no edges to follow; the allowed
/// processors' own products are kept.
fn select_processors(
    graph: &mut crate::graph::BuildGraph,
    allowed: &[String],
    excluded: &[String],
    stop_after: BuildPhase,
) {
    let is_allowed = |p: &crate::graph::Product| allowed.contains(&p.processor);
    if matches!(
        stop_after,
        BuildPhase::Discover | BuildPhase::AddDependencies
    ) {
        graph.retain_products(is_allowed);
        return;
    }
    let selected: std::collections::HashSet<usize> = graph
        .products()
        .iter()
        .filter(|p| is_allowed(p))
        .map(|p| p.id)
        .collect();
    let excluded_ids: std::collections::HashSet<usize> = graph
        .products()
        .iter()
        .filter(|p| excluded.contains(&p.processor))
        .map(|p| p.id)
        .collect();
    graph.select_with_producers(&selected, &excluded_ids);
}

/// Everything discovery produced, ready to classify and execute.
///
/// This is the intermediate that finding 7 said did not exist: previously
/// nothing sat between "run the whole build" and "execute one product", so
/// the planning half could not be reused or inspected without going through
/// `Builder::build`'s CLI-shaped surface. `plan_build` produces one of these;
/// `build` consumes it.
struct BuildPlan {
    processors: ProcessorMap,
    graph: crate::graph::BuildGraph,
    phase_timings: Vec<(String, Duration)>,
    /// Kept for re-analyzing sources that products regenerate mid-build.
    analysis: Option<crate::analyzers::Analysis>,
}

impl Builder {
    /// Apply CLI overrides that mutate config or context before anything is
    /// created from them.
    ///
    /// Separated from `build` because these are CLI *semantics*, not build
    /// steps: they translate flags into the config/context state the rest of
    /// the pipeline reads, and they must all happen before `create_processors`
    /// observes the config.
    fn apply_cli_overrides(
        &mut self,
        ctx: &crate::build_context::BuildContext,
        opts: &BuildOptions,
    ) {
        // CLI override for zspell and aspell auto_add_words
        if opts.auto_add_words {
            for inst in &mut self.config.processor.instances {
                if (inst.pname == "processor.checker.zspell"
                    || inst.pname == "processor.checker.aspell")
                    && let Some(table) = inst.config_toml.as_table_mut()
                {
                    table.insert("auto_add_words".to_string(), toml::Value::Boolean(true));
                }
            }
        }

        // CLI override for mtime pre-check
        if opts.no_mtime {
            ctx.set_mtime_check(false);
        }

        // Apply the configured argv-length threshold (build.max_arg_len) so
        // run_checker can read it via ctx.max_arg_len().
        ctx.set_max_arg_len(self.config.build.max_arg_len);
        ctx.set_command_timeout_secs(self.config.build.command_timeout_secs);
    }

    /// Discover → filter → validate: everything between "we have a config" and
    /// "we have a graph ready to classify".
    ///
    /// Returns the processor map (needed later for execution), the graph, and
    /// the phase timings collected so far. The processor filter is consumed
    /// entirely within this phase, and the resulting graph already reflects
    /// it. Every processor is discovered, and `-p`/`-x` then select from
    /// the graph (see `select_processors`): the selection needs the whole
    /// graph to find the products that generate a selected product's inputs.
    ///
    /// The tool preflight lives here because *when* it runs depends on how the
    /// graph came out: in shared-config mode it is deferred until after
    /// discovery so a processor with no work in this repo doesn't demand its
    /// tool be installed.
    ///
    /// Writes no outputs and advances no baseline (it may warm the dependency
    /// and mtime caches): `--dry-run` plans with it too, so a preview selects
    /// exactly what a build would.
    fn plan_build(
        &self,
        ctx: &crate::build_context::BuildContext,
        opts: &BuildOptions,
    ) -> Result<BuildPlan, anyhow::Error> {
        let t = Instant::now();
        let processors = self.create_processors()?;
        let create_processors_dur = t.elapsed();

        let expanded_filter = resolve_processor_filter(
            opts.processor_filter.as_deref(),
            opts.exclude_filter.as_deref(),
            &processors,
        )?;
        let excluded_processors: Vec<String> = opts
            .exclude_filter
            .as_deref()
            .map(|f| expand_aliases(f, &processors))
            .unwrap_or_default();

        // Build the dependency graph (may stop early based on stop_after)
        let super::GraphBuild {
            mut graph,
            mut phase_timings,
            analysis,
        } = self.build_graph_with_processors_impl(
            ctx,
            &processors,
            super::GraphBuildMode::Normal,
            opts.stop_after,
            opts.verbose,
        )?;

        if let Some(filter) = &expanded_filter {
            select_processors(&mut graph, filter, &excluded_processors, opts.stop_after);
        }

        // Verify required tools — after graph construction, so only processors
        // that actually produced products are checked. A declared processor
        // matching no files in this repo needs no tool installed, which is what
        // lets one shared rsconstruct.toml serve repos with different layouts.
        // Disabled instances (`enabled = false`) are exempt — disabling a
        // processor exists precisely to keep its stanza while its tool is absent.
        // After the -p/-x selection: a producer pulled in for a selected
        // processor runs, so its tool is needed too.
        let with_products: std::collections::HashSet<&str> = graph
            .products()
            .iter()
            .map(|p| p.processor.as_str())
            .collect();
        check_required_tools(&processors, None, Some(&with_products))?;

        // Filter by target patterns if specified
        if let Some(ref targets) = opts.targets {
            graph.filter_by_targets(targets)?;
        }

        phase_timings.insert(0, ("create_processors".to_string(), create_processors_dur));
        Ok(BuildPlan {
            processors,
            graph,
            phase_timings,
            analysis,
        })
    }

    /// Execute an incremental build using the dependency graph.
    ///
    /// Three phases: [`apply_cli_overrides`](Self::apply_cli_overrides)
    /// translates flags into config/context state, [`plan_build`](Self::plan_build)
    /// produces a [`BuildPlan`], and the body below classifies and executes it.
    pub fn build(
        &mut self,
        ctx: &crate::build_context::BuildContext,
        opts: &BuildOptions,
        init_timings: Vec<(String, Duration)>,
    ) -> Result<(), anyhow::Error> {
        self.apply_cli_overrides(ctx, opts);

        let BuildPlan {
            processors,
            mut graph,
            mut phase_timings,
            mut analysis,
        } = self.plan_build(ctx, opts)?;

        // Check for config changes and display diffs. This advances the stored
        // baseline, so it belongs to a real build, not to `plan_build` (which
        // `--dry-run` shares).
        self.detect_config_changes(&processors, opts.show_all_config_changes);

        // Prepend init timings ahead of create_processors, which plan_build
        // already inserted at the front.
        for (i, timing) in init_timings.into_iter().enumerate() {
            phase_timings.insert(i, timing);
        }

        // If we stopped early (before classify), we're done
        if opts.stop_after != BuildPhase::Build && opts.stop_after != BuildPhase::Classify {
            if crate::json_output::human_output_enabled() {
                println!("Stopped after {:?} phase.", opts.stop_after);
            }
            return Ok(());
        }

        // Phase: Classify products (skip/restore/build). Printed in two lines,
        // matching the dep-scan phase style:
        //   - forward-looking total before classify runs (this is the checksum
        //     pass, which is the expensive work for large graphs)
        //   - post-classify breakdown showing what will actually be built
        if phases_debug() {
            eprintln!("{}", color::dim("  Phase: classify"));
        }
        let t = Instant::now();
        let order = graph.topological_sort()?;
        if crate::json_output::human_output_enabled() {
            println!("[build] {} products to check for updates", order.len());
        }
        let policy = crate::executor::IncrementalPolicy;
        let classification = crate::executor::classify_products(
            ctx,
            &policy,
            &graph,
            &order,
            &self.object_store,
            opts.force,
        );
        phase_timings.push(("classify".to_string(), t.elapsed()));
        if crate::json_output::human_output_enabled() {
            println!(
                "[build] {} to build, {} to restore ({} up-to-date)",
                classification.build_count, classification.restore_count, classification.skip_count
            );
        }
        print_graph_stats(GraphSnapshot::AfterClassify, &graph);

        if opts.stop_after == BuildPhase::Classify {
            return Ok(());
        }

        // Stale outputs are removed by the executor: per product right before
        // it restores or rebuilds, and at the end of the run for products
        // predicted to change that never ran (so a failed upstream still
        // leaves no stale downstream output behind).

        // Create executor with parallelism from command line, env var, or config
        let parallel = opts
            .jobs
            .or_else(|| {
                std::env::var("RSCONSTRUCT_THREADS")
                    .ok()
                    .and_then(|v| v.parse().ok())
            })
            .unwrap_or(self.config.build.parallel);
        let effective_parallel = if parallel == 0 {
            std::thread::available_parallelism().map_or(1, std::num::NonZero::get)
        } else {
            parallel
        };
        if crate::json_output::human_output_enabled() {
            println!("[rsconstruct] using {effective_parallel} threads");
        }
        // CLI overrides config for batch_size (CLI -1 maps to None = disable)
        let batch_size = opts
            .batch_size
            .unwrap_or(Some(self.config.build.batch_size));
        let executor = Executor::new(
            &processors,
            ctx,
            &policy,
            ExecutorOptions {
                parallel: effective_parallel,
                verbose: opts.verbose,
                display_opts: opts.display_opts,
                batch_size,
                explain: opts.explain,
                retry: opts.retry,
                force: opts.force,
                keep_going: opts.keep_going,
                // Enable timings collection if trace output is requested
                timings: opts.timings || opts.trace.is_some(),
            },
        );

        // A product that regenerates a source some analyzer scans makes the
        // graph-time scan of that source stale; re-analyze its consumers
        // before they run.
        let mut reanalyze = analysis.as_mut().map(|analysis| {
            move |graph: &mut crate::graph::BuildGraph, changed: &[usize]| {
                analysis.reanalyze(ctx, graph, changed)
            }
        });
        let refresh = reanalyze
            .as_mut()
            .map(|f| f as &mut crate::executor::GraphRefresh<'_>);

        // Execute the build
        let t = Instant::now();
        let result = executor.execute(&mut graph, &self.object_store, &classification, refresh);
        let build_dur = t.elapsed();
        print_graph_stats(GraphSnapshot::AfterExecute, &graph);

        // Exit if interrupted
        if ctx.is_interrupted() {
            return Err(crate::exit_code::RsconstructError::new(
                crate::exit_code::RsconstructExitCode::Interrupted,
                "Build interrupted",
            )
            .into());
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
            )
            .into());
        }

        Ok(())
    }

    /// Show what a build with these options would do, without executing
    /// anything: the same plan (`-p`, `-x`, `--target`, overrides) and the
    /// same prediction (`classify_products`) as [`build`](Self::build), so a
    /// product downstream of a change is shown as BUILD, not SKIP.
    pub fn dry_run(
        &mut self,
        ctx: &crate::build_context::BuildContext,
        opts: &BuildOptions,
    ) -> anyhow::Result<()> {
        self.apply_cli_overrides(ctx, opts);
        let BuildPlan { graph, .. } = self.plan_build(ctx, opts)?;

        let order = graph.topological_sort()?;
        if order.is_empty() {
            println!("No products discovered.");
            return Ok(());
        }

        let policy = crate::executor::IncrementalPolicy;
        let classification = crate::executor::classify_products(
            ctx,
            &policy,
            &graph,
            &order,
            &self.object_store,
            opts.force,
        );
        let entries: Vec<_> = classification
            .products
            .iter()
            .map(|c| {
                let product = graph.get_product(c.id).expect(errors::INVALID_PRODUCT_ID);
                let action = if c.dep_changed {
                    ExplainAction::Rebuild(RebuildReason::DependencyChanged)
                } else if c.input_checksum.is_empty() {
                    ExplainAction::Rebuild(RebuildReason::InputUnreadable)
                } else {
                    policy.explain(
                        ctx,
                        product,
                        &self.object_store,
                        &c.input_checksum,
                        opts.force,
                    )
                };
                (product, action)
            })
            .collect();

        let labels = ProductStatusLabels {
            current: (color::dim("SKIP"), "skip"),
            restorable: (color::cyan("RESTORE"), "restore"),
            stale: (color::yellow("BUILD"), "build"),
            new: (color::yellow("BUILD"), "build-new"),
        };

        Self::print_product_status(
            &entries,
            &StatusPrintOptions {
                labels: &labels,
                explain: opts.explain,
                display_opts: DisplayOptions::default(),
                verbose: true,
                all_processor_names: &[],
                native_processors: &std::collections::HashSet::new(),
                rust_processors: &std::collections::HashSet::new(),
            },
        );
        Ok(())
    }

    /// Show the status of each product in the build graph
    pub fn status(
        &self,
        ctx: &crate::build_context::BuildContext,
        verbose: bool,
        breakdown: bool,
    ) -> anyhow::Result<()> {
        let processors = self.create_processors()?;
        let graph = self.build_graph_with_processors(ctx, &processors)?;

        let products: Vec<&_> = graph.products().iter().collect();
        if products.is_empty() && processors.is_empty() {
            println!("No products discovered.");
            return Ok(());
        }

        let labels = ProductStatusLabels {
            current: (color::green("UP-TO-DATE"), "up-to-date"),
            restorable: (color::cyan("RESTORABLE"), "restorable"),
            stale: (color::yellow("STALE"), "stale"),
            new: (color::magenta("NEW"), "new"),
        };

        // Collect all processor names so we also show processors with 0 files
        let all_proc_names: Vec<&str> = super::sorted_keys(&processors)
            .into_iter()
            .map(std::string::String::as_str)
            .collect();
        let native_set: std::collections::HashSet<&str> = processors
            .iter()
            .filter(|(name, _)| crate::registries::processor::is_native(name.as_str()))
            .map(|(name, _)| name.as_str())
            .collect();
        let rust_set: std::collections::HashSet<&str> = processors
            .iter()
            .filter(|(name, _)| crate::registries::processor::is_rust(name.as_str()))
            .map(|(name, _)| name.as_str())
            .collect();
        let entries: Vec<_> = products
            .iter()
            .map(|product| (*product, self.current_action(ctx, product)))
            .collect();
        Self::print_product_status(
            &entries,
            &StatusPrintOptions {
                labels: &labels,
                explain: false,
                display_opts: DisplayOptions::default(),
                verbose,
                all_processor_names: &all_proc_names,
                native_processors: &native_set,
                rust_processors: &rust_set,
            },
        );

        if breakdown {
            // Collect unique source files per processor, then count by extension
            let mut per_processor_files: BTreeMap<
                &str,
                std::collections::HashSet<&std::path::Path>,
            > = BTreeMap::new();
            // Seed with all processors so 0-file processors are shown
            for name in &all_proc_names {
                per_processor_files.entry(name).or_default();
            }
            for product in &products {
                let files = per_processor_files.entry(&product.processor).or_default();
                for input in &product.inputs {
                    files.insert(input.as_path());
                }
            }
            let mut per_processor: BTreeMap<&str, BTreeMap<String, usize>> = BTreeMap::new();
            for (proc_name, files) in &per_processor_files {
                let ext_counts = per_processor.entry(proc_name).or_default();
                for path in files {
                    let ext = path
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("(no ext)");
                    *ext_counts.entry(ext.to_string()).or_default() += 1;
                }
            }
            crate::output::info("");
            crate::output::info(&format!("{}:", color::bold("Source files by processor")));
            let rows: Vec<Vec<String>> = per_processor
                .iter()
                .map(|(proc_name, ext_counts)| {
                    let total: usize = ext_counts.values().sum();
                    let breakdown_str = if total == 0 {
                        String::new()
                    } else {
                        ext_counts
                            .iter()
                            .map(|(ext, count)| format!("{count} .{ext}"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    };
                    vec![
                        proc_name.to_string(),
                        format!("{} files", total),
                        breakdown_str,
                    ]
                })
                .collect();
            tables::print_table(&["Processor", "Files", "Breakdown"], &rows);
        }

        Ok(())
    }

    /// Show source file counts by extension.
    pub fn info_source(&self, ctx: &crate::build_context::BuildContext) -> anyhow::Result<()> {
        let processors = self.create_processors()?;
        let graph = self.build_graph_with_processors(ctx, &processors)?;

        let products = graph.products();
        let mut all_inputs: std::collections::HashSet<&std::path::Path> =
            std::collections::HashSet::new();
        for product in products {
            for input in &product.inputs {
                all_inputs.insert(input.as_path());
            }
        }

        let mut ext_counts: BTreeMap<String, usize> = BTreeMap::new();
        for path in &all_inputs {
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("(no ext)");
            *ext_counts.entry(ext.to_string()).or_default() += 1;
        }

        if crate::json_output::is_json_mode() {
            let json = serde_json::json!({
                "total": all_inputs.len(),
                "by_extension": ext_counts,
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&json).expect(crate::errors::JSON_SERIALIZE)
            );
        } else {
            println!(
                "{}: {}",
                color::bold("Total source files"),
                all_inputs.len()
            );
            let rows: Vec<Vec<String>> = ext_counts
                .iter()
                .map(|(ext, count)| vec![format!(".{}", ext), count.to_string()])
                .collect();
            tables::print_table(&["Extension", "Count"], &rows);
        }
        Ok(())
    }

    /// What the cache says about a product as it stands now, ignoring what
    /// other products would do first. This is `status`; `--dry-run` predicts
    /// through `classify_products` instead.
    fn current_action(
        &self,
        ctx: &crate::build_context::BuildContext,
        product: &crate::graph::Product,
    ) -> ExplainAction {
        match crate::checksum::combined_input_checksum(ctx, &product.inputs) {
            Ok(input_checksum) => self.object_store.explain_descriptor(
                ctx,
                &product.descriptor_key(&input_checksum),
                &product.outputs,
                false,
            ),
            Err(_) => ExplainAction::Rebuild(RebuildReason::InputUnreadable),
        }
    }

    /// Print the status of each product, with per-processor and total summary.
    /// When `verbose` is false, only the per-processor and total summary lines are printed.
    fn print_product_status(
        entries: &[(&crate::graph::Product, ExplainAction)],
        opts: &StatusPrintOptions<'_>,
    ) {
        const NUM_STATES: usize = 4; // current, restorable, stale, new
        let mut counts = [0usize; NUM_STATES];
        let mut per_processor: BTreeMap<&str, [usize; NUM_STATES]> = BTreeMap::new();
        // Seed with all processor names so processors with 0 products are shown
        for name in opts.all_processor_names {
            per_processor.entry(name).or_default();
        }

        let status_labels = [
            &opts.labels.current.0,
            &opts.labels.restorable.0,
            &opts.labels.stale.0,
            &opts.labels.new.0,
        ];

        for (product, action) in entries {
            let display = product.display(opts.display_opts);
            // Same classification with and without --explain — the flag only
            // adds the reason text.
            let reason = if opts.explain {
                format!(" ({action})")
            } else {
                String::new()
            };
            // Without a descriptor key (an input is unreadable), stale and new
            // are indistinguishable; report as new.
            let status_idx = match action {
                ExplainAction::Skip => 0,
                ExplainAction::Restore(_) => 1,
                ExplainAction::Rebuild(
                    RebuildReason::NoCacheEntry | RebuildReason::InputUnreadable,
                ) => 3,
                ExplainAction::Rebuild(_) => 2,
            };

            if opts.verbose {
                println!(
                    "{} [{}] {}{}",
                    status_labels[status_idx], product.processor, display, reason
                );
            }

            counts[status_idx] += 1;
            per_processor.entry(&product.processor).or_default()[status_idx] += 1;
        }

        if crate::json_output::is_json_mode() {
            let processors_json: Vec<serde_json::Value> = per_processor
                .iter()
                .map(|(name, pc)| {
                    serde_json::json!({
                        "name": name,
                        "up_to_date": pc[0],
                        "restorable": pc[1],
                        "stale": pc[2],
                        "new": pc[3],
                        "total": pc[0] + pc[1] + pc[2] + pc[3],
                        "native": opts.native_processors.contains(name),
                        "rust": opts.rust_processors.contains(name),
                    })
                })
                .collect();
            let json = serde_json::json!({
                "processor": processors_json,
                "totals": {
                    "up_to_date": counts[0],
                    "restorable": counts[1],
                    "stale": counts[2],
                    "new": counts[3],
                    "total": counts[0] + counts[1] + counts[2] + counts[3],
                },
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&json).expect(crate::errors::JSON_SERIALIZE)
            );
            return;
        }

        // Per-processor table
        let col_labels = [
            opts.labels.current.1,
            opts.labels.restorable.1,
            opts.labels.stale.1,
            opts.labels.new.1,
        ];

        let rows: Vec<Vec<String>> = per_processor
            .iter()
            .map(|(name, pc)| {
                let native = crate::tables::yes_no(opts.native_processors.contains(name));
                let rust = crate::tables::yes_no(opts.rust_processors.contains(name));
                vec![
                    name.to_string(),
                    pc[0].to_string(),
                    pc[1].to_string(),
                    pc[2].to_string(),
                    pc[3].to_string(),
                    native.to_string(),
                    rust.to_string(),
                ]
            })
            .collect();
        let total = vec![
            "Total".to_string(),
            counts[0].to_string(),
            counts[1].to_string(),
            counts[2].to_string(),
            counts[3].to_string(),
            String::new(),
            String::new(),
        ];
        tables::print_table_with_total(
            &[
                "Processor",
                col_labels[0],
                col_labels[1],
                col_labels[2],
                col_labels[3],
                "native",
                "rust",
            ],
            &rows,
            &total,
        );
    }
}

/// Write a Chrome trace format JSON file from build statistics.
/// The file can be opened in <chrome://tracing> or <https://ui.perfetto.dev>
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
            let ts_us = pt.start_offset.map_or(0, |off| off.as_micros() as i64);
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
    let trace_json = serde_json::to_string_pretty(&trace)?;
    std::fs::write(path, trace_json)
        .with_context(|| format!("Failed to write trace file: {path}"))?;
    if crate::json_output::human_output_enabled() {
        println!("Wrote trace to {}", color::bold(path));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::create_all_default_processors;

    /// No filters means "run everything", which downstream code distinguishes
    /// from an empty allow-list ("run nothing").
    #[test]
    fn no_filters_means_no_restriction() {
        let procs = create_all_default_processors().expect("default processors");
        let filter = resolve_processor_filter(None, None, &procs).expect("no filters is valid");
        assert!(
            filter.is_none(),
            "expected None (run everything), got {filter:?}"
        );
    }

    /// `-x` alone has to be turned into an allow-list, because everything
    /// downstream consumes an include list rather than an exclude list.
    #[test]
    fn exclude_only_synthesizes_an_include_list() {
        let procs = create_all_default_processors().expect("default processors");
        let exclude = vec!["processor.checker.ruff".to_string()];
        let filter = resolve_processor_filter(None, Some(&exclude), &procs)
            .expect("exclude-only is valid")
            .expect("exclude-only must synthesize a list");
        assert!(
            !filter.contains(&"processor.checker.ruff".to_string()),
            "excluded processor must not survive"
        );
        assert!(
            filter.len() > 1,
            "expected everything-but-ruff, got {} entries",
            filter.len()
        );
        assert_eq!(
            filter.len(),
            procs.len() - 1,
            "exactly one processor should be removed"
        );
    }

    /// `-p` wins the intersection: an include list is narrowed by excludes.
    #[test]
    fn include_is_narrowed_by_exclude() {
        let procs = create_all_default_processors().expect("default processors");
        let include = vec![
            "processor.checker.ruff".to_string(),
            "processor.checker.mypy".to_string(),
        ];
        let exclude = vec!["processor.checker.black".to_string()];
        let filter = resolve_processor_filter(Some(&include), Some(&exclude), &procs)
            .expect("disjoint include/exclude is valid")
            .expect("include must produce a list");
        // expand_aliases sorts and dedups, so compare as a set.
        let mut got = filter;
        got.sort();
        assert_eq!(
            got,
            vec![
                "processor.checker.mypy".to_string(),
                "processor.checker.ruff".to_string()
            ]
        );
    }

    /// A name in both -p and -x is contradictory intent and must fail rather
    /// than silently resolving to one side.
    #[test]
    fn conflicting_include_and_exclude_errors() {
        let procs = create_all_default_processors().expect("default processors");
        let both = vec!["processor.checker.ruff".to_string()];
        let err = resolve_processor_filter(Some(&both), Some(&both), &procs)
            .expect_err("same name in -p and -x must be rejected");
        let msg = format!("{err}");
        assert!(msg.contains("both -p and -x"), "unexpected message: {msg}");
    }

    /// An unknown name is a typo, not an empty selection — report it, and
    /// report it from whichever flag it appeared in.
    #[test]
    fn unknown_names_error_from_either_filter() {
        let procs = create_all_default_processors().expect("default processors");
        let bogus = vec!["definitely-not-a-processor".to_string()];

        let err = resolve_processor_filter(Some(&bogus), None, &procs)
            .expect_err("unknown -p name must be rejected");
        assert!(format!("{err}").contains("definitely-not-a-processor"));

        let err = resolve_processor_filter(None, Some(&bogus), &procs)
            .expect_err("unknown -x name must be rejected");
        assert!(format!("{err}").contains("definitely-not-a-processor"));
    }

    fn expand(alias: &str, procs: &ProcessorMap) -> Vec<String> {
        expand_aliases(&[alias.to_string()], procs)
    }

    /// `@<short name>` selects that processor. It used to compare the bare
    /// word against full names and never matched, so `@tera` (not a tool
    /// name) was reported as an unknown processor.
    #[test]
    fn short_name_alias_selects_the_processor() {
        let procs = create_all_default_processors().expect("default processors");
        assert_eq!(expand("@tera", &procs), vec!["processor.generator.tera"]);
        assert!(expand("@ruff", &procs).contains(&"processor.checker.ruff".to_string()));
    }

    /// A short name shared across types selects every processor carrying
    /// it, and a short name selects the processor's instances too.
    #[test]
    fn short_name_alias_spans_types_and_instances() {
        let mut procs = create_all_default_processors().expect("default processors");
        let generic = expand("@generic", &procs);
        for pname in [
            "processor.checker.generic",
            "processor.creator.generic",
            "processor.generator.generic",
            "processor.explicit.generic",
        ] {
            if procs.contains_key(pname) {
                assert!(
                    generic.contains(&pname.to_string()),
                    "{pname} in {generic:?}"
                );
            }
        }
        assert!(generic.len() > 1, "{generic:?}");

        let tera = procs.remove("processor.generator.tera").unwrap();
        procs.insert("processor.generator.tera.docs".to_string(), tera);
        assert_eq!(
            expand("@tera", &procs),
            vec!["processor.generator.tera.docs"]
        );
    }

    /// Every processor type is an alias, `@explicit` included (it used to be
    /// missing from the hand-written list), and selects only that type.
    #[test]
    fn type_aliases_cover_every_type() {
        let procs = create_all_default_processors().expect("default processors");
        let explicit = expand("@explicit", &procs);
        assert_ne!(explicit.len(), 0, "@explicit must select something");
        assert!(
            explicit
                .iter()
                .all(|n| n.starts_with("processor.explicit.")),
            "{explicit:?}"
        );
        let checkers = expand("@checker", &procs);
        assert!(checkers.iter().all(|n| n.starts_with("processor.checker.")));
        assert!(checkers.contains(&"processor.checker.ruff".to_string()));
    }

    /// An alias that selects nothing stays as written, so validation names
    /// it as unknown.
    #[test]
    fn unmatched_alias_is_reported_unknown() {
        let procs = create_all_default_processors().expect("default processors");
        let filter = vec!["@definitely-not-anything".to_string()];
        let err = resolve_processor_filter(Some(&filter), None, &procs)
            .expect_err("unmatched alias must be rejected");
        assert!(format!("{err}").contains("@definitely-not-anything"));
    }
}
