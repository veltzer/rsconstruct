use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use crate::analyzers::{CppDepAnalyzer, DepAnalyzer, PythonDepAnalyzer};
use crate::cli::{BuildOptions, BuildPhase, ConfigAction, DepsAction, DisplayOptions, GraphFormat, GraphViewer, ProcessorAction, ToolsAction};
use crate::color;
use crate::config::Config;
use crate::deps_cache::DepsCache;
use crate::executor::{Executor, ExecutorOptions};
use crate::file_index::FileIndex;
use crate::graph::BuildGraph;
use crate::object_store::{ObjectStore, ObjectStoreOptions};
use crate::processors::{CargoProcessor, CcProcessor, ClangTidyProcessor, CppcheckProcessor, LuaProcessor, MakeProcessor, PylintProcessor, RuffProcessor, ShellcheckProcessor, ProductDiscovery, SleepProcessor, SpellcheckProcessor, TeraProcessor, log_command};
use crate::remote_cache;
use crate::tool_lock;

/// Severity level for validation issues.
pub enum ValidationSeverity {
    Error,
    Warning,
}

/// A single validation issue found during config validation.
pub struct ValidationIssue {
    pub severity: ValidationSeverity,
    pub message: String,
}

/// Global flag: when true, print phase messages during graph building.
static PHASES_DEBUG: AtomicBool = AtomicBool::new(false);

/// Enable phases debug logging (called once from main).
pub fn set_phases_debug(enabled: bool) {
    PHASES_DEBUG.store(enabled, Ordering::Relaxed);
}

/// Check if phases debug is enabled.
fn phases_debug() -> bool {
    PHASES_DEBUG.load(Ordering::Relaxed)
}

/// Labels for the three product states used by dry_run and status.
struct ProductStatusLabels {
    current: (String, &'static str),
    restorable: (String, &'static str),
    stale: (String, &'static str),
}

pub struct Builder {
    project_root: PathBuf,
    object_store: ObjectStore,
    config: Config,
    file_index: FileIndex,
}

impl Builder {
    pub fn new() -> Result<Self> {
        let project_root = std::env::current_dir()?;
        Config::require_config(&project_root)?;
        let config = Config::load(&project_root)?;

        // Create remote cache backend if configured
        let remote_backend = match &config.cache.remote {
            Some(url) => Some(remote_cache::create_backend(url)?),
            None => None,
        };

        let object_store = ObjectStore::new(ObjectStoreOptions {
            restore_method: config.cache.restore_method,
            remote: remote_backend,
            remote_push: config.cache.remote_push,
            remote_pull: config.cache.remote_pull,
            mtime_check: config.cache.mtime_check,
        })?;
        let file_index = FileIndex::build(&project_root)?;

        Ok(Self {
            project_root,
            object_store,
            config,
            file_index,
        })
    }

    /// Execute an incremental build using the dependency graph
    pub fn build(&mut self, opts: &BuildOptions, interrupted: Arc<std::sync::atomic::AtomicBool>) -> Result<()> {
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
        let processors = self.create_processors(opts.verbose)?;

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
        let graph = self.build_graph_with_processors_and_phase(&processors, opts.stop_after, processor_filter)?;

        // If we stopped early, we're done
        if opts.stop_after != BuildPhase::Build {
            println!("Stopped after {:?} phase.", opts.stop_after);
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
            progress: opts.progress,
            explain: opts.explain,
        }, Arc::clone(&interrupted));

        // Execute the build
        let result = executor.execute(&graph, &self.object_store, opts.force, opts.timings, opts.keep_going);

        // Always save object store index, even after errors or interrupt
        self.object_store.save()?;

        // Exit after saving if interrupted
        if interrupted.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(crate::exit_code::RsbError::new(
                crate::exit_code::RsbExitCode::Interrupted,
                "Build interrupted",
            ).into());
        }

        let stats = result?;

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
    pub fn dry_run(&self, force: bool, explain: bool) -> Result<()> {
        let processors = self.create_processors(false)?;
        let graph = self.build_graph_with_processors(&processors)?;

        let order = graph.topological_sort()?;
        if order.is_empty() {
            println!("No products discovered.");
            return Ok(());
        }

        let products: Vec<_> = order.iter()
            .map(|&id| graph.get_product(id).expect("internal error: invalid product id"))
            .collect();

        let labels = ProductStatusLabels {
            current: (color::dim("SKIP"), "skip"),
            restorable: (color::cyan("RESTORE"), "restore"),
            stale: (color::yellow("BUILD"), "build"),
        };

        self.print_product_status(&products, force, &labels, explain);
        Ok(())
    }

    /// Show the status of each product in the build graph
    pub fn status(&self) -> Result<()> {
        let processors = self.create_processors(false)?;
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

        self.print_product_status(&products, false, &labels, false);
        Ok(())
    }

    /// Classify and print the status of each product, with a summary line.
    fn print_product_status(
        &self,
        products: &[&crate::graph::Product],
        force: bool,
        labels: &ProductStatusLabels,
        explain: bool,
    ) {
        use crate::object_store::ExplainAction;

        let mut counts = [0usize; 3]; // [current, restorable, stale]

        let display_opts = DisplayOptions::minimal();
        for product in products {
            let cache_key = product.cache_key();
            let input_checksum = match self.object_store.combined_input_checksum_fast(&product.inputs) {
                Ok(cs) => cs,
                Err(_) => {
                    println!("{} [{}] {}", labels.stale.0, product.processor, product.display(display_opts));
                    counts[2] += 1;
                    continue;
                }
            };

            if explain {
                let action = self.object_store.explain_action(&cache_key, &input_checksum, &product.outputs, force);
                let reason_str = format!(" ({})", action);
                match action {
                    ExplainAction::Skip => {
                        println!("{} [{}] {}{}", labels.current.0, product.processor, product.display(display_opts), reason_str);
                        counts[0] += 1;
                    }
                    ExplainAction::Restore(_) => {
                        println!("{} [{}] {}{}", labels.restorable.0, product.processor, product.display(display_opts), reason_str);
                        counts[1] += 1;
                    }
                    ExplainAction::Rebuild(_) => {
                        println!("{} [{}] {}{}", labels.stale.0, product.processor, product.display(display_opts), reason_str);
                        counts[2] += 1;
                    }
                }
            } else if !force && !self.object_store.needs_rebuild(&cache_key, &input_checksum, &product.outputs) {
                println!("{} [{}] {}", labels.current.0, product.processor, product.display(display_opts));
                counts[0] += 1;
            } else if !force && self.object_store.can_restore(&cache_key, &input_checksum, &product.outputs) {
                println!("{} [{}] {}", labels.restorable.0, product.processor, product.display(display_opts));
                counts[1] += 1;
            } else {
                println!("{} [{}] {}", labels.stale.0, product.processor, product.display(display_opts));
                counts[2] += 1;
            }
        }

        println!();
        println!("{}: {} {}, {} {}, {} {}",
            color::bold("Summary"),
            counts[0], labels.current.1,
            counts[1], labels.restorable.1,
            counts[2], labels.stale.1);
    }

    /// Clean all build artifacts using the dependency graph
    pub fn clean(&self) -> Result<()> {
        println!("{}", color::bold("Cleaning build artifacts..."));

        // Create processors and build graph (fast path: skip dependency scanning)
        let processors = self.create_processors(false)?;
        let graph = self.build_graph_for_clean_with_processors(&processors)?;

        // Use executor to clean (batch_size doesn't matter for clean)
        let executor = Executor::new(&processors, ExecutorOptions {
            parallel: 1,
            verbose: false,
            display_opts: DisplayOptions::minimal(),
            batch_size: None,
            progress: false,
            explain: false,
        }, Arc::new(std::sync::atomic::AtomicBool::new(false)));
        executor.clean(&graph)?;

        // Remove empty subdirectories under out/
        let out_dir = self.project_root.join("out");
        if out_dir.is_dir() {
            for entry in fs::read_dir(&out_dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_dir() && fs::read_dir(&path)?.next().is_none() {
                    fs::remove_dir(&path)
                        .context(format!("Failed to remove directory {}", path.display()))?;
                    println!("Removed empty directory: {}", path.display());
                }
            }
        }

        println!("{}", color::green("Clean completed!"));
        Ok(())
    }

    /// Remove all build outputs and cache directories (.rsb/ and out/)
    pub fn distclean(&self) -> Result<()> {
        println!("{}", color::bold("Removing build directories..."));

        let rsb_dir = self.project_root.join(".rsb");
        if rsb_dir.exists() {
            fs::remove_dir_all(&rsb_dir)
                .context("Failed to remove .rsb/ directory")?;
            println!("Removed {}", rsb_dir.display());
        }

        let out_dir = self.project_root.join("out");
        if out_dir.exists() {
            fs::remove_dir_all(&out_dir)
                .context("Failed to remove out/ directory")?;
            println!("Removed {}", out_dir.display());
        }

        println!("{}", color::green("Distclean completed!"));
        Ok(())
    }

    /// Hard clean using `git clean -qffxd`. Requires a git repository.
    pub fn hardclean(&self) -> Result<()> {
        let git_dir = self.project_root.join(".git");
        if !git_dir.exists() {
            bail!("Not a git repository. Hardclean requires a git repository.");
        }

        println!("{}", color::bold("Running git clean -qffxd..."));

        let output = Command::new("git")
            .args(["clean", "-qffxd"])
            .current_dir(&self.project_root)
            .output()
            .context("Failed to run git clean")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("git clean failed:\n{}", stderr);
        }

        println!("{}", color::green("Hardclean completed!"));
        Ok(())
    }

    /// Create all available processors
    pub fn create_processors(&self, verbose: bool) -> Result<HashMap<String, Box<dyn ProductDiscovery>>> {
        let mut processors: HashMap<String, Box<dyn ProductDiscovery>> = HashMap::new();

        // Tera processor
        if let Ok(tera_proc) = TeraProcessor::new(self.project_root.clone(), self.config.processor.tera.clone()) {
            processors.insert("tera".to_string(), Box::new(tera_proc));
        }

        // Ruff processor
        let ruff_proc = RuffProcessor::new(self.project_root.clone(), self.config.processor.ruff.clone());
        processors.insert("ruff".to_string(), Box::new(ruff_proc));

        // Pylint processor
        let pylint_proc = PylintProcessor::new(self.project_root.clone(), self.config.processor.pylint.clone());
        processors.insert("pylint".to_string(), Box::new(pylint_proc));

        // Sleep processor (for testing parallelism)
        let sleep_proc = SleepProcessor::new(self.project_root.clone(), self.config.processor.sleep.clone());
        processors.insert("sleep".to_string(), Box::new(sleep_proc));

        // C/C++ compiler processor (single-file compilation)
        let cc_proc = CcProcessor::new(self.project_root.clone(), self.config.processor.cc_single_file.clone(), verbose);
        processors.insert("cc_single_file".to_string(), Box::new(cc_proc));

        // C/C++ static analysis processor (cppcheck)
        let cppcheck_proc = CppcheckProcessor::new(self.project_root.clone(), self.config.processor.cppcheck.clone());
        processors.insert("cppcheck".to_string(), Box::new(cppcheck_proc));

        // C/C++ static analysis processor (clang-tidy)
        let clang_tidy_proc = ClangTidyProcessor::new(self.project_root.clone(), self.config.processor.clang_tidy.clone());
        processors.insert("clang_tidy".to_string(), Box::new(clang_tidy_proc));

        // Shellcheck processor
        let shellcheck_proc = ShellcheckProcessor::new(self.project_root.clone(), self.config.processor.shellcheck.clone());
        processors.insert("shellcheck".to_string(), Box::new(shellcheck_proc));

        // Spellcheck processor
        match SpellcheckProcessor::new(self.project_root.clone(), self.config.processor.spellcheck.clone()) {
            Ok(spellcheck_proc) => {
                processors.insert("spellcheck".to_string(), Box::new(spellcheck_proc));
            }
            Err(e) => {
                if self.config.processor.is_enabled("spellcheck") {
                    return Err(e);
                }
            }
        }

        // Make processor
        let make_proc = MakeProcessor::new(self.project_root.clone(), self.config.processor.make.clone());
        processors.insert("make".to_string(), Box::new(make_proc));

        // Cargo processor
        let cargo_proc = CargoProcessor::new(self.project_root.clone(), self.config.processor.cargo.clone());
        processors.insert("cargo".to_string(), Box::new(cargo_proc));

        // Lua plugin processors
        let lua_plugins = LuaProcessor::discover_plugins(
            &self.project_root,
            &self.config.plugins.dir,
            &self.config.processor.extra,
        )?;
        for (name, proc) in lua_plugins {
            if processors.contains_key(&name) {
                anyhow::bail!("Lua plugin '{}' conflicts with built-in processor", name);
            }
            processors.insert(name, Box::new(proc));
        }

        Ok(processors)
    }

    /// Detect and display config changes for each processor.
    /// Shows colored diffs when processor configuration has changed since last build.
    fn detect_config_changes(&self, processors: &HashMap<String, Box<dyn ProductDiscovery>>) {
        // Don't show config diffs in JSON mode
        if crate::json_output::is_json_mode() {
            return;
        }

        let mut processor_names: Vec<_> = processors.keys().collect();
        processor_names.sort();

        for name in processor_names {
            let processor = processors.get(name).unwrap();

            // Skip processors that don't provide config JSON
            let config_json = match processor.config_json() {
                Some(json) => json,
                None => continue,
            };

            // Store the config and check if it changed
            if let Ok(Some(old_json)) = self.object_store.store_processor_config(name, &config_json) {
                // Config changed - show diff
                if let Some(diff) = ObjectStore::diff_configs(&old_json, &config_json) {
                    println!("{}",
                        color::yellow(&format!("Config changed for [{}]:", name)));
                    println!("{}", diff);
                }
            }
        }
    }

    /// Create all available dependency analyzers
    fn create_analyzers(&self, verbose: bool) -> HashMap<String, Box<dyn DepAnalyzer>> {
        let mut analyzers: HashMap<String, Box<dyn DepAnalyzer>> = HashMap::new();

        // C/C++ dependency analyzer
        let cpp_analyzer = CppDepAnalyzer::new(
            self.config.analyzer.cpp.clone(),
            self.project_root.clone(),
            verbose,
        );
        analyzers.insert("cpp".to_string(), Box::new(cpp_analyzer));

        // Python dependency analyzer
        let python_analyzer = PythonDepAnalyzer::new(self.project_root.clone());
        analyzers.insert("python".to_string(), Box::new(python_analyzer));

        analyzers
    }

    /// Print the dependency graph in the specified format
    pub fn print_graph(&self, format: GraphFormat) -> Result<()> {
        let graph = self.build_graph()?;

        // Output in the requested format
        let output = match format {
            GraphFormat::Dot => graph.to_dot(),
            GraphFormat::Mermaid => graph.to_mermaid(),
            GraphFormat::Json => graph.to_json(),
            GraphFormat::Text => graph.to_text(),
            GraphFormat::Svg => graph.to_svg()?,
        };

        println!("{}", output);
        Ok(())
    }

    /// View the dependency graph in a viewer
    pub fn view_graph(&self, viewer: GraphViewer) -> Result<()> {
        use std::process::Command;

        let graph = self.build_graph()?;

        // Create temp file
        let temp_dir = std::env::temp_dir();

        match viewer {
            GraphViewer::Mermaid => {
                let html_path = temp_dir.join("rsb_graph.html");
                let html_content = graph.to_html();
                fs::write(&html_path, html_content)
                    .context("Failed to write HTML file")?;

                // Open in browser
                self.open_file(&html_path)?;
                println!("Opened graph in browser: {}", html_path.display());
            }
            GraphViewer::Svg => {
                // Check if dot is available
                let mut dot_check_cmd = Command::new("dot");
                dot_check_cmd.arg("-V");
                log_command(&dot_check_cmd);
                let dot_check = dot_check_cmd.output();
                if dot_check.is_err() || !dot_check.unwrap().status.success() {
                    anyhow::bail!("Graphviz 'dot' command not found. Install Graphviz or use --view=mermaid");
                }

                let dot_path = temp_dir.join("rsb_graph.dot");
                let svg_path = temp_dir.join("rsb_graph.svg");

                // Write DOT file
                let dot_content = graph.to_dot();
                fs::write(&dot_path, dot_content)
                    .context("Failed to write DOT file")?;

                // Convert to SVG
                let mut dot_cmd = Command::new("dot");
                dot_cmd.arg("-Tsvg").arg(&dot_path).arg("-o").arg(&svg_path);
                log_command(&dot_cmd);
                let output = dot_cmd
                    .output()
                    .context("Failed to run dot command")?;

                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    anyhow::bail!("dot command failed: {}", stderr);
                }

                // Open SVG
                self.open_file(&svg_path)?;
                println!("Opened graph: {}", svg_path.display());
            }
        }

        Ok(())
    }

    /// Run all enabled dependency analyzers on the graph
    fn run_analyzers(&self, graph: &mut BuildGraph) -> Result<()> {
        let analyzers = self.create_analyzers(false);
        let mut deps_cache = DepsCache::open()?;

        // Collect which analyzers should run
        let mut names: Vec<&String> = analyzers.keys().collect();
        names.sort();

        let active_analyzers: Vec<&String> = names.into_iter()
            .filter(|name| {
                let in_enabled_list = self.config.analyzer.is_enabled(name);
                if !in_enabled_list {
                    return false;
                }
                !self.config.analyzer.auto_detect || analyzers[*name].auto_detect(&self.file_index)
            })
            .collect();

        // Run each analyzer
        for name in &active_analyzers {
            analyzers[*name].analyze(graph, &mut deps_cache, &self.file_index)?;
        }

        // Flush the cache
        let _ = deps_cache.flush();

        Ok(())
    }

    /// Build the dependency graph using provided processors
    fn build_graph_with_processors(&self, processors: &HashMap<String, Box<dyn ProductDiscovery>>) -> Result<BuildGraph> {
        self.build_graph_with_processors_impl(processors, false, BuildPhase::Build, None)
    }

    /// Build the dependency graph with optional early stopping
    fn build_graph_with_processors_and_phase(&self, processors: &HashMap<String, Box<dyn ProductDiscovery>>, stop_after: BuildPhase, processor_filter: Option<&[String]>) -> Result<BuildGraph> {
        self.build_graph_with_processors_impl(processors, false, stop_after, processor_filter)
    }

    /// Build the dependency graph for clean (skip expensive dependency scanning)
    fn build_graph_for_clean_with_processors(&self, processors: &HashMap<String, Box<dyn ProductDiscovery>>) -> Result<BuildGraph> {
        self.build_graph_with_processors_impl(processors, true, BuildPhase::Build, None)
    }

    /// Build the dependency graph using provided processors
    /// processor_filter: if Some, only run processors in this list (in addition to enabled check)
    fn build_graph_with_processors_impl(&self, processors: &HashMap<String, Box<dyn ProductDiscovery>>, for_clean: bool, stop_after: BuildPhase, processor_filter: Option<&[String]>) -> Result<BuildGraph> {
        if phases_debug() {
            eprintln!("{}", color::bold("Phase: Building dependency graph..."));
        }
        let mut graph = BuildGraph::new();

        // Collect which processors should run
        let mut names: Vec<&String> = processors.keys().collect();
        names.sort();
        let active_processors: Vec<&String> = names.into_iter()
            .filter(|name| {
                // If filter is specified, only include processors in the filter list
                if let Some(filter) = processor_filter
                    && !filter.iter().any(|f| f == *name) {
                        return false;
                    }
                let in_enabled_list = self.config.processor.is_enabled(name);
                if !in_enabled_list {
                    return false;
                }
                !self.config.processor.auto_detect || processors[*name].auto_detect(&self.file_index)
            })
            .collect();

        // Phase 1: Discover products
        if phases_debug() {
            eprintln!("{}", color::dim("  Phase: discover"));
        }
        for name in &active_processors {
            if for_clean {
                processors[*name].discover_for_clean(&mut graph, &self.file_index)?;
            } else {
                processors[*name].discover(&mut graph, &self.file_index)?;
            }
        }

        if stop_after == BuildPhase::Discover {
            return Ok(graph);
        }

        // Phase 2: Run dependency analyzers (only for regular builds, not clean)
        if !for_clean {
            if phases_debug() {
                eprintln!("{}", color::dim("  Phase: add_dependencies"));
            }
            self.run_analyzers(&mut graph)?;
        }

        if stop_after == BuildPhase::AddDependencies {
            return Ok(graph);
        }

        // Phase 3: Apply tool version hashes
        if phases_debug() {
            eprintln!("{}", color::dim("  Phase: apply_tool_version_hashes"));
        }
        let config = &self.config;
        let tool_hashes = tool_lock::processor_tool_hashes(
            &self.project_root,
            processors,
            &|name| config.processor.is_enabled(name),
        )?;
        if !tool_hashes.is_empty() {
            graph.apply_tool_version_hashes(&tool_hashes);
        }

        // Phase 4: Resolve dependencies
        if phases_debug() {
            eprintln!("{}", color::dim("  Phase: resolve_dependencies"));
        }
        graph.resolve_dependencies();

        // Note: BuildPhase::Resolve and BuildPhase::Build both complete the graph
        Ok(graph)
    }

    /// Build the dependency graph, optionally filtering to a single processor.
    /// If `include_all` is true, skip enabled/auto-detect checks.
    pub fn build_graph_filtered(
        &self,
        filter_name: Option<&str>,
        include_all: bool,
    ) -> Result<BuildGraph> {
        let processors = self.create_processors(false)?;
        let mut graph = BuildGraph::new();

        let mut names: Vec<&String> = processors.keys().collect();
        names.sort();

        // Collect active processors
        let active_processors: Vec<&String> = names.into_iter()
            .filter(|name| {
                if let Some(filter) = filter_name
                    && name.as_str() != filter {
                        return false;
                    }
                if !include_all {
                    let in_enabled_list = self.config.processor.is_enabled(name);
                    if !in_enabled_list {
                        return false;
                    }
                    let should_run = !self.config.processor.auto_detect || processors[*name].auto_detect(&self.file_index);
                    if !should_run {
                        return false;
                    }
                }
                true
            })
            .collect();

        // Phase 1: Discover products
        for name in &active_processors {
            processors[*name].discover(&mut graph, &self.file_index)?;
        }

        // Phase 2: Run dependency analyzers
        self.run_analyzers(&mut graph)?;

        graph.resolve_dependencies();
        Ok(graph)
    }

    /// Build the dependency graph (creates processors internally)
    fn build_graph(&self) -> Result<BuildGraph> {
        let processors = self.create_processors(false)?;
        self.build_graph_with_processors(&processors)
    }

    /// Build the dependency graph for cache operations (public).
    pub fn build_graph_for_cache(&self) -> Result<BuildGraph> {
        self.build_graph()
    }

    /// Compute the set of valid cache keys from the current build graph.
    pub fn valid_cache_keys(&self) -> Result<std::collections::HashSet<String>> {
        let graph = self.build_graph_for_cache()?;
        Ok(graph.products().iter().map(|p| p.cache_key()).collect())
    }

    /// Get a reference to the object store.
    pub fn object_store(&self) -> &ObjectStore {
        &self.object_store
    }

    /// Get a mutable reference to the object store.
    pub fn object_store_mut(&mut self) -> &mut ObjectStore {
        &mut self.object_store
    }

    /// Handle `rsb processor` subcommands
    pub fn processor(&self, action: ProcessorAction) -> Result<()> {
        let processors = self.create_processors(false)?;

        let mut proc_names: Vec<&String> = processors.keys().collect();
        proc_names.sort();

        match action {
            ProcessorAction::List { all } => {
                for name in &proc_names {
                    let proc = &processors[name.as_str()];
                    if proc.hidden() && !all {
                        continue;
                    }
                    let status = if self.config.processor.is_enabled(name) {
                        color::green("enabled")
                    } else {
                        color::dim("disabled")
                    };
                    let proc_type = color::dim(&format!("[{}]", proc.processor_type().as_str()));
                    let batch = if proc.supports_batch() {
                        format!(" {}", color::dim("[batch]"))
                    } else {
                        String::new()
                    };
                    println!("{} {}{} {}", name, proc_type, batch, status);
                }
            }
            ProcessorAction::All => {
                for name in &proc_names {
                    let proc = &processors[name.as_str()];
                    let enabled_status = if self.config.processor.is_enabled(name) {
                        color::green("enabled")
                    } else {
                        color::dim("disabled")
                    };
                    let hidden_status = if proc.hidden() {
                        format!(" {}", color::dim("(hidden)"))
                    } else {
                        String::new()
                    };
                    let proc_type = color::dim(&format!("[{}]", proc.processor_type().as_str()));
                    let batch = if proc.supports_batch() {
                        format!(" {}", color::dim("[batch]"))
                    } else {
                        String::new()
                    };
                    println!("{} {}{} {}{} \u{2014} {}", name, proc_type, batch, enabled_status, hidden_status, color::dim(proc.description()));
                }
            }
            ProcessorAction::Auto => {
                for name in &proc_names {
                    let proc = &processors[name.as_str()];
                    let detected = proc.auto_detect(&self.file_index);
                    let enabled = self.config.processor.is_enabled(name);
                    let status = match (detected, enabled) {
                        (true, true) => color::green("detected, enabled"),
                        (true, false) => color::yellow("detected, disabled"),
                        (false, true) => color::yellow("not detected, enabled"),
                        (false, false) => color::dim("not detected, disabled"),
                    };
                    println!("{:<12} {}", name, status);
                }
            }
            ProcessorAction::Files { name, all } => {
                if let Some(ref n) = name
                    && !processors.contains_key(n.as_str()) {
                        bail!("Unknown processor: '{}'. Run 'rsb processor list' to see available processors.", n);
                    }

                let graph = self.build_graph_filtered(name.as_deref(), all)?;

                let products = graph.products();

                if crate::json_output::is_json_mode() {
                    let entries: Vec<crate::json_output::ProcessorFileEntry> = products.iter()
                        .map(|p| {
                            let proc_type = if p.outputs.is_empty() { "checker" } else { "generator" };
                            crate::json_output::ProcessorFileEntry {
                                processor: p.processor.clone(),
                                processor_type: proc_type.to_string(),
                                inputs: p.inputs.iter().map(|i| i.display().to_string()).collect(),
                                outputs: p.outputs.iter().map(|o| o.display().to_string()).collect(),
                            }
                        })
                        .collect();
                    println!("{}", serde_json::to_string_pretty(&entries)?);
                    return Ok(());
                }

                if products.is_empty() {
                    if let Some(ref n) = name {
                        println!("[{}] (no files)", n);
                    } else {
                        println!("No files discovered by any processor.");
                    }
                    return Ok(());
                }

                let mut counts: HashMap<&str, usize> = HashMap::new();
                for p in products {
                    *counts.entry(p.processor.as_str()).or_insert(0) += 1;
                }

                let mut current_processor = "";
                for product in products {
                    if product.processor.as_str() != current_processor {
                        if !current_processor.is_empty() {
                            println!();
                        }
                        current_processor = product.processor.as_str();
                        let n = counts[current_processor];
                        println!("[{}] ({} {})", current_processor, n, if n == 1 { "product" } else { "products" });
                    }
                    let inputs: Vec<String> = product.inputs.iter()
                        .map(|p| p.display().to_string())
                        .collect();
                    // For checkers (empty outputs), display "(checker)" instead of output paths
                    if product.outputs.is_empty() {
                        println!("{} \u{2192} {}", inputs.join(", "), color::dim("(checker)"));
                    } else {
                        let outputs: Vec<String> = product.outputs.iter()
                            .map(|p| p.display().to_string())
                            .collect();
                        println!("{} \u{2192} {}", inputs.join(", "), outputs.join(", "));
                    }
                }
            }
        }

        Ok(())
    }

    /// Verify tool versions against .tools.versions lock file.
    /// Called at the start of build unless --ignore-tool-versions is passed.
    pub fn verify_tool_versions(&self) -> Result<()> {
        let processors = self.create_processors(false)?;
        let config = &self.config;
        let tool_commands = tool_lock::collect_tool_commands(
            &processors,
            &|name| config.processor.is_enabled(name),
        );
        if tool_commands.is_empty() {
            return Ok(());
        }
        tool_lock::verify_lock_file(&self.project_root, &tool_commands)
    }

    /// Handle `rsb tools` subcommands
    pub fn tools(&self, action: ToolsAction) -> Result<()> {
        let processors = self.create_processors(false)?;

        let show_all = matches!(&action, ToolsAction::List { all: true } | ToolsAction::Check { all: true });

        let mut tool_pairs: Vec<(String, String)> = Vec::new();
        let mut names: Vec<&String> = processors.keys().collect();
        names.sort();
        for name in names {
            if !show_all && !self.config.processor.is_enabled(name) {
                continue;
            }
            for tool in processors[name].required_tools() {
                tool_pairs.push((tool, name.clone()));
            }
        }
        tool_pairs.sort();
        tool_pairs.dedup();

        match action {
            ToolsAction::List { .. } => {
                for (tool, processor) in &tool_pairs {
                    println!("{} ({})", tool, processor);
                }
            }
            ToolsAction::Check { .. } => {
                let mut any_missing = false;
                for (tool, processor) in &tool_pairs {
                    if let Ok(path) = which::which(tool) {
                        println!("{} ({}) {} {}", tool, processor, color::green("found"), color::dim(&path.display().to_string()));
                    } else {
                        println!("{} ({}) {}", tool, processor, color::red("missing"));
                        any_missing = true;
                    }
                }
                if any_missing {
                    return Err(crate::exit_code::RsbError::new(
                        crate::exit_code::RsbExitCode::ToolError,
                        "Some required tools are missing",
                    ).into());
                }
            }
            ToolsAction::Lock { check } => {
                let config = &self.config;
                let tool_commands = tool_lock::collect_tool_commands(
                    &processors,
                    &|name| config.processor.is_enabled(name),
                );

                if check {
                    tool_lock::verify_lock_file(&self.project_root, &tool_commands)?;
                    println!("{}", color::green("Tool versions match lock file."));
                } else {
                    let lock = tool_lock::create_lock(&tool_commands)?;
                    for (name, info) in &lock.tools {
                        let first_line = info.version_output.lines().next().unwrap_or("");
                        println!("{} {} {}", name, color::green("locked"), color::dim(first_line));
                    }
                    tool_lock::write_lock_file(&self.project_root, &lock)?;
                    println!("Wrote {}", color::bold(".tools.versions"));
                }
            }
        }

        Ok(())
    }

    /// Handle `rsb config` subcommands
    pub fn config(&self, action: ConfigAction) -> Result<()> {
        match action {
            ConfigAction::Show => {
                let output = toml::to_string_pretty(&self.config)?;
                let annotated = Self::annotate_config(&output);
                println!("{}", annotated);
            }
            ConfigAction::ShowDefault => {
                let config = Config::default();
                let output = toml::to_string_pretty(&config)?;
                let annotated = Self::annotate_config(&output);
                println!("{}", annotated);
            }
            ConfigAction::Validate => {
                let issues = self.validate_config();

                if crate::json_output::is_json_mode() {
                    let json_issues: Vec<serde_json::Value> = issues.iter()
                        .map(|issue| {
                            let severity = match issue.severity {
                                ValidationSeverity::Error => "error",
                                ValidationSeverity::Warning => "warning",
                            };
                            serde_json::json!({
                                "severity": severity,
                                "message": issue.message,
                            })
                        })
                        .collect();
                    println!("{}", serde_json::to_string_pretty(&json_issues)?);
                } else if issues.is_empty() {
                    println!("{}", color::green("Config OK"));
                } else {
                    for issue in &issues {
                        let label = match issue.severity {
                            ValidationSeverity::Error => color::red("ERROR"),
                            ValidationSeverity::Warning => color::yellow("WARNING"),
                        };
                        println!("{}: {}", label, issue.message);
                    }
                    let error_count = issues.iter()
                        .filter(|i| matches!(i.severity, ValidationSeverity::Error))
                        .count();
                    let warning_count = issues.iter()
                        .filter(|i| matches!(i.severity, ValidationSeverity::Warning))
                        .count();
                    println!();
                    println!("{}: {} error(s), {} warning(s)",
                        color::bold("Summary"), error_count, warning_count);

                    if error_count > 0 {
                        return Err(crate::exit_code::RsbError::new(
                            crate::exit_code::RsbExitCode::ConfigError,
                            format!("Config validation failed with {} error(s)", error_count),
                        ).into());
                    }
                }
            }
        }

        Ok(())
    }

    /// Validate the configuration and return all issues found.
    fn validate_config(&self) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();

        // Check 1: Enabled processor names are valid
        let processors = match self.create_processors(false) {
            Ok(p) => p,
            Err(e) => {
                issues.push(ValidationIssue {
                    severity: ValidationSeverity::Error,
                    message: format!("Failed to create processors: {}", e),
                });
                return issues;
            }
        };

        for name in &self.config.processor.enabled {
            if !processors.contains_key(name) {
                issues.push(ValidationIssue {
                    severity: ValidationSeverity::Error,
                    message: format!("Unknown processor '{}' in enabled list", name),
                });
            }
        }

        // Check 2: Required tools on PATH for enabled processors
        for name in &self.config.processor.enabled {
            if let Some(processor) = processors.get(name) {
                for tool in processor.required_tools() {
                    if which::which(&tool).is_err() {
                        issues.push(ValidationIssue {
                            severity: ValidationSeverity::Warning,
                            message: format!("Tool '{}' required by processor '{}' not found on PATH", tool, name),
                        });
                    }
                }
            }
        }

        // Check 3: Auto-detect mismatch (processor enabled but no matching files)
        for name in &self.config.processor.enabled {
            if let Some(processor) = processors.get(name)
                && !processor.auto_detect(&self.file_index)
            {
                issues.push(ValidationIssue {
                    severity: ValidationSeverity::Warning,
                    message: format!("Processor '{}' is enabled but no matching files detected", name),
                });
            }
        }

        issues
    }

    /// Annotate TOML config output with comments for constrained values
    fn annotate_config(toml: &str) -> String {
        toml.lines()
            .map(|line| {
                let trimmed = line.trim();
                if trimmed.starts_with("parallel = ") {
                    format!("{} # 0 = auto-detect CPU cores", line)
                } else if trimmed.starts_with("restore_method = ") {
                    format!("{} # options: hardlink, copy", line)
                } else {
                    line.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Open a file with the configured viewer or the system default application
    fn open_file(&self, path: &std::path::Path) -> Result<()> {
        use std::process::Command;

        let cmd = if let Some(ref viewer) = self.config.graph.viewer {
            viewer.as_str()
        } else {
            #[cfg(target_os = "linux")]
            { "xdg-open" }

            #[cfg(target_os = "macos")]
            { "open" }

            #[cfg(target_os = "windows")]
            { "start" }
        };

        let mut open_cmd = Command::new(cmd);
        open_cmd.arg(path);
        log_command(&open_cmd);
        open_cmd
            .spawn()
            .context(format!("Failed to open file with {}", cmd))?;

        Ok(())
    }

    /// Handle `rsb deps` subcommands
    pub fn deps(&self, action: DepsAction) -> Result<()> {
        use crate::deps_cache::DepsCache;

        match action {
            DepsAction::List => {
                // List all available dependency analyzers
                let analyzers = self.create_analyzers(false);
                let mut names: Vec<&String> = analyzers.keys().collect();
                names.sort();

                for name in names {
                    let analyzer = &analyzers[name];
                    let enabled = self.config.analyzer.is_enabled(name);
                    let detected = analyzer.auto_detect(&self.file_index);

                    let status = match (enabled, detected) {
                        (true, true) => color::green("enabled, detected"),
                        (true, false) => color::yellow("enabled, not detected"),
                        (false, true) => color::yellow("disabled, detected"),
                        (false, false) => color::dim("disabled"),
                    };

                    println!("{:<10} {} — {}", name, status, color::dim(analyzer.description()));
                }
            }
            DepsAction::Clean { analyzer } => {
                if let Some(analyzer_name) = analyzer {
                    // Clear only entries from specific analyzer
                    let deps_cache = DepsCache::open()?;
                    let removed = deps_cache.remove_by_analyzer(&analyzer_name)?;
                    deps_cache.flush()?;
                    if removed > 0 {
                        println!("Removed {} entries from '{}' analyzer.", removed, analyzer_name);
                    } else {
                        println!("No entries found for '{}' analyzer.", analyzer_name);
                    }
                } else {
                    // Clear the entire dependency cache
                    let deps_file = self.project_root.join(".rsb").join("deps.redb");
                    if deps_file.exists() {
                        fs::remove_file(&deps_file)
                            .context("Failed to remove dependency cache")?;
                        println!("Dependency cache cleared.");
                    } else {
                        println!("Dependency cache is already empty.");
                    }
                }
            }
            DepsAction::Stats => {
                // Show statistics by analyzer
                let deps_cache = DepsCache::open()?;
                let stats = deps_cache.stats_by_analyzer();
                if stats.is_empty() {
                    println!("Dependency cache is empty. Run a build first.");
                    return Ok(());
                }
                let mut total_files = 0;
                let mut total_deps = 0;
                let mut names: Vec<_> = stats.keys().collect();
                names.sort();
                for name in names {
                    let (files, deps) = stats[name];
                    total_files += files;
                    total_deps += deps;
                    println!("{}: {} files, {} dependencies",
                        color::bold(name), files, deps);
                }
                println!();
                println!("{}: {} files, {} dependencies",
                    color::bold("Total"), total_files, total_deps);
            }
            DepsAction::Show { filter } => {
                use crate::cli::DepsShowFilter;
                let deps_cache = DepsCache::open()?;

                match filter {
                    DepsShowFilter::All => {
                        // List all entries
                        let mut entries: Vec<_> = deps_cache.list_all();
                        if entries.is_empty() {
                            println!("Dependency cache is empty. Run a build first.");
                            return Ok(());
                        }
                        // Sort by source path for consistent output
                        entries.sort_by(|a, b| a.0.cmp(&b.0));
                        for (source, deps, analyzer) in entries {
                            Self::print_deps(&source, &deps, &analyzer);
                        }
                    }
                    DepsShowFilter::Files { files } => {
                        // Query specific files
                        let mut found_any = false;
                        for file_arg in &files {
                            let file_path = PathBuf::from(file_arg);
                            if let Some((deps, analyzer)) = deps_cache.get_raw(&file_path) {
                                found_any = true;
                                Self::print_deps(&file_path, &deps, &analyzer);
                            } else {
                                eprintln!("{}: '{}' not in dependency cache", color::yellow("Warning"), file_arg);
                            }
                        }
                        if !found_any {
                            bail!("No cached dependencies found for the specified files");
                        }
                    }
                    DepsShowFilter::Analyzers { analyzers } => {
                        // Filter by analyzer names
                        let mut entries: Vec<_> = deps_cache.list_by_analyzers(&analyzers);
                        if entries.is_empty() {
                            println!("No cached dependencies found for analyzers: {}", analyzers.join(", "));
                            return Ok(());
                        }
                        // Sort by source path for consistent output
                        entries.sort_by(|a, b| a.0.cmp(&b.0));
                        for (source, deps, analyzer) in entries {
                            Self::print_deps(&source, &deps, &analyzer);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Print dependencies for a source file
    fn print_deps(source: &std::path::Path, deps: &[PathBuf], analyzer: &str) {
        let analyzer_tag = if analyzer.is_empty() {
            String::new()
        } else {
            format!(" {}", color::dim(&format!("[{}]", analyzer)))
        };
        if deps.is_empty() {
            println!("{}:{} {}", source.display(), analyzer_tag, color::dim("(no dependencies)"));
        } else {
            println!("{}:{}", source.display(), analyzer_tag);
            for dep in deps {
                println!("  {}", dep.display());
            }
        }
    }
}
