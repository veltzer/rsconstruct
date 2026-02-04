use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use crate::analyzers::{CppDepAnalyzer, DepAnalyzer, PythonDepAnalyzer};
use crate::cli::{BuildPhase, ConfigAction, DepsAction, GraphFormat, GraphViewer, ProcessorAction, ToolsAction};
use crate::color;
use crate::config::Config;
use crate::deps_cache::DepsCache;
use crate::executor::Executor;
use crate::file_index::FileIndex;
use crate::graph::BuildGraph;
use crate::object_store::ObjectStore;
use crate::processors::{CcProcessor, CpplintProcessor, LuaProcessor, MakeProcessor, PylintProcessor, RuffProcessor, ShellcheckProcessor, ProductDiscovery, SleepProcessor, SpellcheckProcessor, TemplateProcessor, log_command};
use crate::remote_cache;
use crate::tool_lock;

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

        let object_store = ObjectStore::new(
            config.cache.restore_method,
            remote_backend,
            config.cache.remote_push,
            config.cache.remote_pull,
        )?;
        let file_index = FileIndex::build(&project_root)?;

        Ok(Self {
            project_root,
            object_store,
            config,
            file_index,
        })
    }

    /// Execute an incremental build using the dependency graph
    /// batch_size_override: Some(Some(n)) = use n, Some(None) = disable batching, None = use config
    pub fn build(&self, force: bool, verbose: bool, file_names: u8, jobs: Option<usize>, timings: bool, keep_going: bool, interrupted: Arc<std::sync::atomic::AtomicBool>, summary: bool, batch_size_override: Option<Option<usize>>, stop_after: BuildPhase) -> Result<()> {
        // Create processors
        let processors = self.create_processors(verbose)?;

        // Build the dependency graph (may stop early based on stop_after)
        let graph = self.build_graph_with_processors_and_phase(&processors, stop_after)?;

        // If we stopped early, we're done
        if stop_after != BuildPhase::Build {
            println!("Stopped after {:?} phase.", stop_after);
            return Ok(());
        }

        // Create executor with parallelism from command line or config
        let parallel = jobs.unwrap_or(self.config.build.parallel);
        // CLI overrides config for batch_size
        let batch_size = batch_size_override.unwrap_or(self.config.build.batch_size);
        let executor = Executor::new(&processors, parallel, verbose, file_names, Arc::clone(&interrupted), batch_size);

        // Execute the build
        let result = executor.execute(&graph, &self.object_store, force, timings, keep_going);

        // Always save object store index, even after errors or interrupt
        self.object_store.save()?;

        // Exit after saving if interrupted
        if interrupted.load(std::sync::atomic::Ordering::SeqCst) {
            std::process::exit(130);
        }

        let stats = result?;

        // Print summary
        stats.print_summary(summary, timings);

        // Return error if there were failures in keep-going mode
        if stats.failed_count > 0 {
            anyhow::bail!("Build completed with {} error(s)", stats.failed_count);
        }

        Ok(())
    }

    /// Show what would happen without executing anything
    pub fn dry_run(&self, force: bool) -> Result<()> {
        let processors = self.create_processors(false)?;
        let graph = self.build_graph_with_processors(&processors)?;

        let order = graph.topological_sort()?;
        if order.is_empty() {
            println!("No products discovered.");
            return Ok(());
        }

        let products: Vec<_> = order.iter()
            .map(|&id| graph.get_product(id).unwrap())
            .collect();

        let labels = ProductStatusLabels {
            current: (color::dim("SKIP"), "skip"),
            restorable: (color::cyan("RESTORE"), "restore"),
            stale: (color::yellow("BUILD"), "build"),
        };

        self.print_product_status(&products, force, &labels);
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

        self.print_product_status(&products, false, &labels);
        Ok(())
    }

    /// Classify and print the status of each product, with a summary line.
    fn print_product_status(
        &self,
        products: &[&crate::graph::Product],
        force: bool,
        labels: &ProductStatusLabels,
    ) {
        let mut counts = [0usize; 3]; // [current, restorable, stale]

        for product in products {
            let cache_key = product.cache_key();
            let input_checksum = match ObjectStore::combined_input_checksum(&product.inputs) {
                Ok(cs) => cs,
                Err(_) => {
                    println!("{} [{}] {}", labels.stale.0, product.processor, product.display(0));
                    counts[2] += 1;
                    continue;
                }
            };

            if !force && !self.object_store.needs_rebuild(&cache_key, &input_checksum, &product.outputs) {
                println!("{} [{}] {}", labels.current.0, product.processor, product.display(0));
                counts[0] += 1;
            } else if !force && self.object_store.can_restore(&cache_key, &input_checksum, &product.outputs) {
                println!("{} [{}] {}", labels.restorable.0, product.processor, product.display(0));
                counts[1] += 1;
            } else {
                println!("{} [{}] {}", labels.stale.0, product.processor, product.display(0));
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
        let executor = Executor::new(&processors, 1, false, 0, Arc::new(std::sync::atomic::AtomicBool::new(false)), None);
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

        // Template processor
        if let Ok(template_proc) = TemplateProcessor::new(self.project_root.clone(), self.config.processor.template.clone()) {
            processors.insert("template".to_string(), Box::new(template_proc));
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

        // C/C++ lint processor
        let cpplinter = CpplintProcessor::new(self.project_root.clone(), self.config.processor.cpplint.clone());
        processors.insert("cpplint".to_string(), Box::new(cpplinter));

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
        self.build_graph_with_processors_impl(processors, false, BuildPhase::Build)
    }

    /// Build the dependency graph with optional early stopping
    fn build_graph_with_processors_and_phase(&self, processors: &HashMap<String, Box<dyn ProductDiscovery>>, stop_after: BuildPhase) -> Result<BuildGraph> {
        self.build_graph_with_processors_impl(processors, false, stop_after)
    }

    /// Build the dependency graph for clean (skip expensive dependency scanning)
    fn build_graph_for_clean_with_processors(&self, processors: &HashMap<String, Box<dyn ProductDiscovery>>) -> Result<BuildGraph> {
        self.build_graph_with_processors_impl(processors, true, BuildPhase::Build)
    }

    /// Build the dependency graph using provided processors
    fn build_graph_with_processors_impl(&self, processors: &HashMap<String, Box<dyn ProductDiscovery>>, for_clean: bool, stop_after: BuildPhase) -> Result<BuildGraph> {
        if phases_debug() {
            eprintln!("{}", color::bold("Phase: Building dependency graph..."));
        }
        let mut graph = BuildGraph::new();

        // Collect which processors should run
        let mut names: Vec<&String> = processors.keys().collect();
        names.sort();
        let active_processors: Vec<&String> = names.into_iter()
            .filter(|name| {
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
                if let Some(filter) = filter_name {
                    if name.as_str() != filter {
                        return false;
                    }
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
                if let Some(ref n) = name {
                    if !processors.contains_key(n.as_str()) {
                        bail!("Unknown processor: '{}'. Run 'rsb processor list' to see available processors.", n);
                    }
                }

                let graph = self.build_graph_filtered(name.as_deref(), all)?;

                let products = graph.products();
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
                    bail!("Some required tools are missing");
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
        }

        Ok(())
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
            DepsAction::Clean => {
                // Clear the dependency cache
                let deps_dir = self.project_root.join(".rsb").join("deps");
                if deps_dir.exists() {
                    fs::remove_dir_all(&deps_dir)
                        .context("Failed to remove dependency cache")?;
                    println!("Dependency cache cleared.");
                } else {
                    println!("Dependency cache is already empty.");
                }
            }
            DepsAction::All => {
                // Open the dependency cache and list all entries
                let deps_cache = DepsCache::open()?;
                let mut entries: Vec<_> = deps_cache.list_all();
                if entries.is_empty() {
                    println!("Dependency cache is empty. Run a build first.");
                    return Ok(());
                }
                // Sort by source path for consistent output
                entries.sort_by(|a, b| a.0.cmp(&b.0));
                for (source, deps) in entries {
                    Self::print_deps(&source, &deps);
                }
            }
            DepsAction::For { files } => {
                // Open the dependency cache and query specific files
                let deps_cache = DepsCache::open()?;
                let mut found_any = false;
                for file_arg in &files {
                    let file_path = PathBuf::from(file_arg);
                    if let Some(deps) = deps_cache.get_raw(&file_path) {
                        found_any = true;
                        Self::print_deps(&file_path, &deps);
                    } else {
                        eprintln!("{}: '{}' not in dependency cache", color::yellow("Warning"), file_arg);
                    }
                }
                if !found_any {
                    bail!("No cached dependencies found for the specified files");
                }
            }
        }

        Ok(())
    }

    /// Print dependencies for a source file
    fn print_deps(source: &std::path::Path, deps: &[PathBuf]) {
        if deps.is_empty() {
            println!("{}: {}", source.display(), color::dim("(no dependencies)"));
        } else {
            println!("{}:", source.display());
            for dep in deps {
                println!("  {}", dep.display());
            }
        }
    }
}
