mod build;
mod clean;
mod config_cmd;
mod deps;
mod doctor;
mod graph;
pub(crate) mod processors;
pub(crate) mod sloc;
pub(crate) mod smart;
pub(crate) mod symlink_install;
pub(crate) mod tools;

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use anyhow::Result;
use crate::analyzers::{CppDepAnalyzer, DepAnalyzer, PythonDepAnalyzer};
use crate::cli::{BuildPhase, DisplayOptions};
use crate::color;
#[allow(unused_imports)]
use crate::config::*;
use crate::deps_cache::DepsCache;
use crate::errors;
use crate::file_index::FileIndex;
use crate::graph::BuildGraph;
use crate::object_store::{ObjectStore, ObjectStoreOptions};
use crate::processors as proc_mod;
use crate::processors::{LuaProcessor, ProcessorMap, ProductDiscovery, names as proc_names};
use crate::remote_cache;
use crate::tool_lock;

/// Phase timing data collected during graph building.
pub(crate) type PhaseTimings = Vec<(String, Duration)>;

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

/// Controls which graph-building variant to use.
#[derive(Clone, Copy, PartialEq, Eq)]
enum GraphBuildMode {
    /// Full build: discover products, run analyzers, resolve dependencies
    Normal,
    /// Clean: only discover products to find output files (skip expensive analysis)
    ForClean,
}

/// Return the keys of a HashMap sorted alphabetically.
fn sorted_keys<V>(map: &HashMap<String, V>) -> Vec<&String> {
    let mut keys: Vec<&String> = map.keys().collect();
    keys.sort();
    keys
}

/// Check if phases debug is enabled.
fn phases_debug() -> bool {
    crate::runtime_flags::phases_debug()
}

/// Labels for the three product states used by dry_run and status.
struct ProductStatusLabels<'a> {
    current: (Cow<'a, str>, &'static str),
    restorable: (Cow<'a, str>, &'static str),
    stale: (Cow<'a, str>, &'static str),
}

/// Options for `print_product_status`.
struct StatusPrintOptions<'a> {
    force: bool,
    labels: &'a ProductStatusLabels<'a>,
    explain: bool,
    display_opts: DisplayOptions,
    verbose: bool,
    all_processor_names: &'a [&'a str],
}

/// Auto-generate `create_processor_for_instance` dispatch function from the central registry.
/// Given a type name and TOML config, constructs the appropriate processor.
macro_rules! gen_processor_dispatch {
    ( $( $const_name:ident, $field:ident, $config_type:ty, $proc_type:ident,
         ($scan_dir:expr, $exts:expr, $excl:expr); )* ) => {
        /// Create a processor from a type name and TOML config value.
        /// Returns None for unknown types (Lua plugins handled separately).
        pub(crate) fn create_processor_for_instance(
            type_name: &str,
            config_toml: &toml::Value,
        ) -> anyhow::Result<Option<Box<dyn ProductDiscovery>>> {
            match type_name {
                $(
                    stringify!($field) => {
                        let mut cfg: $config_type = toml::from_str(&toml::to_string(config_toml)?)?;
                        cfg.scan.resolve($scan_dir, $exts, $excl);
                        gen_processor_dispatch!(@construct $proc_type, cfg)
                    }
                )*
                _ => Ok(None), // Unknown type, not a builtin
            }
        }

        /// Create all builtin processors with default configs (used by tools_no_config, list_processors_no_config).
        pub(crate) fn create_all_default_processors() -> ProcessorMap {
            let mut processors: ProcessorMap = HashMap::new();
            $(
                gen_processor_dispatch!(@register_default processors, $const_name, $field, $config_type, $proc_type,
                    $scan_dir, $exts, $excl);
            )*
            processors
        }
    };
    // Construct a processor. new() must return Self (not Result<Self>).
    // This is enforced by the type annotation: if new() returns Result,
    // the assignment to `proc` will fail to compile with a clear error.
    (@construct $proc_type:ident, $cfg:ident) => {{
        let proc: proc_mod::$proc_type = proc_mod::$proc_type::new($cfg);
        Ok(Some(Box::new(proc)))
    }};
    // Register a processor with default config. new() must return Self (not Result<Self>).
    (@register_default $processors:ident, $const_name:ident, $field:ident, $config_type:ty, $proc_type:ident,
     $scan_dir:expr, $exts:expr, $excl:expr) => {
        {
            let mut cfg = <$config_type>::default();
            cfg.scan.resolve($scan_dir, $exts, $excl);
            let proc: proc_mod::$proc_type = proc_mod::$proc_type::new(cfg);
            Builder::register(&mut $processors, proc_names::$const_name, proc);
        }
    };
}
for_each_processor!(gen_processor_dispatch);

pub struct Builder {
    object_store: ObjectStore,
    config: Config,
    file_index: FileIndex,
}

impl Builder {
    pub fn new() -> Result<Self> {
        Config::require_config()?;
        let config = Config::load()?;

        // Resolve auto restore method based on environment
        let restore_method = config.cache.restore_method.resolve();

        // Validate: compression and hardlink restore are incompatible
        if config.cache.compression && restore_method == crate::config::RestoreMethod::Hardlink {
            anyhow::bail!("Cannot use cache compression with hardlink restore method. \
                Set restore_method = \"copy\" or disable compression.");
        }

        // Create remote cache backend if configured
        let remote_backend = match &config.cache.remote {
            Some(url) => Some(remote_cache::create_backend(url)?),
            None => None,
        };

        let object_store = ObjectStore::new(ObjectStoreOptions {
            restore_method,
            compression: config.cache.compression,
            remote: remote_backend,
            remote_push: config.cache.remote_push,
            remote_pull: config.cache.remote_pull,
            mtime_check: config.cache.mtime_check,
        })?;
        let file_index = FileIndex::build()?;

        Ok(Self {
            object_store,
            config,
            file_index,
        })
    }

    /// Register a processor into the map.
    fn register(processors: &mut ProcessorMap, name: &str, proc: impl ProductDiscovery + 'static) {
        processors.insert(name.to_string(), Box::new(proc));
    }

    /// Create processors from declared instances in the config.
    /// Only processors declared in `[processor.TYPE]` or `[processor.TYPE.NAME]` are created.
    pub fn create_processors(&self) -> Result<ProcessorMap> {
        let cfg = &self.config.processor;
        let mut processors: ProcessorMap = HashMap::new();

        for inst in &cfg.instances {
            match create_processor_for_instance(&inst.type_name, &inst.config_toml) {
                Ok(Some(proc)) => {
                    processors.insert(inst.instance_name.clone(), proc);
                }
                Ok(None) => {
                    anyhow::bail!("Unknown processor type: '{}'", inst.type_name);
                }
                Err(e) => {
                    return Err(e.context(format!(
                        "Failed to create processor instance '{}'", inst.instance_name
                    )));
                }
            }
        }

        // Lua plugin processors
        let lua_plugins = LuaProcessor::discover_plugins(
            &self.config.plugins.dir,
            &cfg.extra,
        )?;
        for (name, proc) in lua_plugins {
            if processors.contains_key(&name) {
                anyhow::bail!("Lua plugin '{}' conflicts with processor instance", name);
            }
            processors.insert(name, Box::new(proc));
        }

        Ok(processors)
    }

    /// Detect and display config changes for each processor.
    /// Shows colored diffs when processor configuration has changed since last build.
    fn detect_config_changes(&self, processors: &ProcessorMap) {
        // Don't show config diffs in JSON or quiet mode
        if crate::json_output::is_json_mode() || crate::runtime_flags::quiet() {
            return;
        }

        for name in sorted_keys(processors) {
            let processor = processors.get(name).expect(errors::PROCESSOR_NOT_IN_MAP);

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
            verbose,
        );
        analyzers.insert("cpp".to_string(), Box::new(cpp_analyzer));

        // Python dependency analyzer
        let python_analyzer = PythonDepAnalyzer::new();
        analyzers.insert("python".to_string(), Box::new(python_analyzer));

        analyzers
    }

    /// Run all enabled dependency analyzers on the graph
    fn run_analyzers(&self, graph: &mut BuildGraph, verbose: bool) -> Result<()> {
        let analyzers = self.create_analyzers(verbose);
        let mut deps_cache = DepsCache::open()?;

        // Collect which analyzers should run
        let active_analyzers: Vec<&String> = sorted_keys(&analyzers).into_iter()
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
            analyzers[*name].analyze(graph, &mut deps_cache, &self.file_index, verbose)?;
        }

        Ok(())
    }

    /// Build the dependency graph using provided processors
    fn build_graph_with_processors(&self, processors: &ProcessorMap) -> Result<BuildGraph> {
        let (graph, _) = self.build_graph_with_processors_impl(processors, GraphBuildMode::Normal, BuildPhase::Build, None, false)?;
        Ok(graph)
    }

    /// Build the dependency graph with optional early stopping
    fn build_graph_with_processors_and_phase(&self, processors: &ProcessorMap, stop_after: BuildPhase, processor_filter: Option<&[String]>, verbose: bool) -> Result<(BuildGraph, PhaseTimings)> {
        self.build_graph_with_processors_impl(processors, GraphBuildMode::Normal, stop_after, processor_filter, verbose)
    }

    /// Build the dependency graph for clean (skip expensive dependency scanning)
    fn build_graph_for_clean_with_processors(&self, processors: &ProcessorMap) -> Result<BuildGraph> {
        let (graph, _) = self.build_graph_with_processors_impl(processors, GraphBuildMode::ForClean, BuildPhase::Build, None, false)?;
        Ok(graph)
    }

    /// Return the set of processor type names whose files are detected in the project.
    /// Uses default configs for all builtin processors to check auto_detect.
    pub fn detected_processors(&self) -> Result<std::collections::HashSet<String>> {
        let processors = create_all_default_processors();
        let mut detected = std::collections::HashSet::new();
        for (name, proc) in &processors {
            if proc.auto_detect(&self.file_index) {
                detected.insert(name.clone());
            }
        }
        Ok(detected)
    }

    /// Return the set of processor type names whose files are detected AND whose
    /// required tools are all installed.
    pub fn detected_and_available_processors(&self) -> Result<std::collections::HashSet<String>> {
        let processors = create_all_default_processors();
        let mut available = std::collections::HashSet::new();
        for (name, proc) in &processors {
            if !proc.auto_detect(&self.file_index) {
                continue;
            }
            let tools = proc.required_tools();
            if tools.iter().all(|t| which::which(t).is_ok()) {
                available.insert(name.clone());
            }
        }
        Ok(available)
    }

    /// Return the set of configured processor instance names that have 0 products
    /// (i.e., don't match any files).
    pub fn no_file_processors(&self) -> Result<Vec<String>> {
        let processors = self.create_processors()?;
        let graph = self.build_graph_with_processors(&processors)?;

        let mut has_products: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for product in graph.products() {
            has_products.insert(product.processor.as_str());
        }

        let mut empty: Vec<String> = sorted_keys(&processors)
            .into_iter()
            .filter(|name| !has_products.contains(name.as_str()))
            .cloned()
            .collect();
        empty.sort();
        Ok(empty)
    }

    /// Check whether a processor should run. In the instance-based model,
    /// all declared processors are active (existence in config = enabled).
    fn is_processor_active(&self, _name: &str, _processor: &dyn ProductDiscovery) -> bool {
        true
    }

    /// Build the instance_name → type_name mapping for named processor instances.
    fn instance_to_type_map(&self) -> HashMap<String, String> {
        self.config.processor.instances.iter()
            .filter(|inst| inst.instance_name != inst.type_name)
            .map(|inst| (inst.instance_name.clone(), inst.type_name.clone()))
            .collect()
    }

    /// Fixed-point product discovery loop.
    /// Runs discovery for all active processors, then injects declared outputs
    /// as virtual files so downstream processors can discover products for files
    /// that don't exist on disk yet. Repeats until no new products are found.
    fn discover_products(
        &self,
        graph: &mut BuildGraph,
        processors: &ProcessorMap,
        active: &[impl AsRef<str>],
        for_clean: bool,
    ) -> Result<()> {
        let instance_to_type = self.instance_to_type_map();
        let mut file_index = self.file_index.clone();
        let debug = phases_debug();

        for pass in 0..10 {
            let before = graph.products().len();
            for name in active {
                let name = name.as_ref();
                if for_clean {
                    processors[name].discover_for_clean(graph, &file_index)?;
                } else {
                    processors[name].discover(graph, &file_index)?;
                }
                if let Some(type_name) = instance_to_type.get(name) {
                    graph.remap_processor_name(type_name, name);
                }
            }
            let after = graph.products().len();
            if after == before {
                break;
            }
            if pass + 1 >= 10 {
                break;
            }
            let outputs: Vec<PathBuf> = graph.products()[before..after]
                .iter()
                .flat_map(|p| p.outputs.iter().cloned())
                .collect();
            let added = file_index.add_virtual_files(&outputs);
            if added == 0 {
                break;
            }
            if debug {
                eprintln!("{}", color::dim(&format!(
                    "    discover pass {}: {} new products, {} virtual files added",
                    pass + 1, after - before, added
                )));
            }
        }
        Ok(())
    }

    /// Build the dependency graph using provided processors
    /// processor_filter: if Some, only run processors in this list (in addition to enabled check)
    fn build_graph_with_processors_impl(&self, processors: &ProcessorMap, mode: GraphBuildMode, stop_after: BuildPhase, processor_filter: Option<&[String]>, verbose: bool) -> Result<(BuildGraph, PhaseTimings)> {
        if phases_debug() {
            eprintln!("{}", color::bold("Phase: Building dependency graph..."));
        }
        let mut graph = BuildGraph::new();
        let mut phase_timings = PhaseTimings::new();

        // Collect which processors should run
        let active_processors: Vec<&String> = sorted_keys(processors).into_iter()
            .filter(|name| {
                if let Some(filter) = processor_filter
                    && !filter.iter().any(|f| f == *name) {
                        return false;
                    }
                self.is_processor_active(name, processors[*name].as_ref())
            })
            .collect();

        // Phase 1: Discover products
        if phases_debug() {
            eprintln!("{}", color::dim("  Phase: discover"));
        }
        let t = Instant::now();
        self.discover_products(&mut graph, processors, &active_processors, mode == GraphBuildMode::ForClean)?;
        phase_timings.push(("discover".to_string(), t.elapsed()));

        if stop_after == BuildPhase::Discover {
            return Ok((graph, phase_timings));
        }

        // Phase 2: Run dependency analyzers (only for regular builds, not clean)
        if mode == GraphBuildMode::Normal {
            if phases_debug() {
                eprintln!("{}", color::dim("  Phase: add_dependencies"));
            }
            let t = Instant::now();
            self.run_analyzers(&mut graph, verbose)?;
            phase_timings.push(("add_dependencies".to_string(), t.elapsed()));
        }

        if stop_after == BuildPhase::AddDependencies {
            return Ok((graph, phase_timings));
        }

        // Phase 3: Apply tool version hashes
        if phases_debug() {
            eprintln!("{}", color::dim("  Phase: apply_tool_version_hashes"));
        }
        let t = Instant::now();
        let tool_hashes = tool_lock::processor_tool_hashes(
            processors,
            &|_name| true, // All declared processors are active
        )?;
        if !tool_hashes.is_empty() {
            graph.apply_tool_version_hashes(&tool_hashes);
        }
        phase_timings.push(("tool_version_hashes".to_string(), t.elapsed()));

        // Phase 4: Resolve dependencies
        if phases_debug() {
            eprintln!("{}", color::dim("  Phase: resolve_dependencies"));
        }
        let t = Instant::now();
        graph.resolve_dependencies();
        phase_timings.push(("resolve".to_string(), t.elapsed()));

        // Note: BuildPhase::Resolve and BuildPhase::Build both complete the graph
        Ok((graph, phase_timings))
    }

    /// Build the dependency graph, optionally filtering to a single processor.
    /// If `include_all` is true, skip enabled/auto-detect checks.
    pub fn build_graph_filtered(
        &self,
        filter_name: Option<&str>,
        include_all: bool,
    ) -> Result<BuildGraph> {
        let processors = self.create_processors()?;
        let mut graph = BuildGraph::new();

        // Collect active processors
        let active_processors: Vec<&String> = sorted_keys(&processors).into_iter()
            .filter(|name| {
                if let Some(filter) = filter_name
                    && name.as_str() != filter {
                        return false;
                    }
                include_all || self.is_processor_active(name, processors[*name].as_ref())
            })
            .collect();

        // Phase 1: Discover products (fixed-point loop for cross-processor deps)
        self.discover_products(&mut graph, &processors, &active_processors, false)?;

        // Phase 2: Run dependency analyzers
        self.run_analyzers(&mut graph, false)?;

        graph.resolve_dependencies();
        Ok(graph)
    }

    /// Build the dependency graph (creates processors internally)
    fn build_graph(&self) -> Result<BuildGraph> {
        let processors = self.create_processors()?;
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

    /// Return directories that should be watched for file changes.
    /// Derived from processor scan configs plus standard project files.
    pub fn watch_paths(&self) -> Vec<PathBuf> {
        let mut paths: Vec<PathBuf> = Vec::new();

        // Always watch rsconstruct.toml
        let config_path = PathBuf::from("rsconstruct.toml");
        if config_path.exists() {
            paths.push(config_path);
        }

        // Add scan directories from all processor configs
        for dir in self.config.processor.scan_dirs() {
            let full = PathBuf::from(&dir);
            if full.exists() {
                paths.push(full);
            }
        }

        // Processors with empty scan_dir scan the project root — watch common
        // top-level files/dirs that wouldn't be covered by scan_dirs above.
        for name in &["pyproject.toml", "config"] {
            let p = PathBuf::from(name);
            if p.exists() {
                paths.push(p);
            }
        }

        // Plugins directory
        let plugins = PathBuf::from(&self.config.plugins.dir);
        if plugins.exists() {
            paths.push(plugins);
        }

        paths.sort();
        paths.dedup();
        paths
    }
}
