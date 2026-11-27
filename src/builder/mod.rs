mod add_config;
pub mod analyzers;
mod build;
pub mod cache_cmd;
mod clean;
mod doctor;
mod graph;
pub mod processor;
mod product;
pub mod sloc;
pub mod smart;
pub mod symlink_install;
pub mod tools;

pub use add_config::{add_analyzer, add_processor};

use crate::analyzers::DepAnalyzer;
use crate::cli::{BuildPhase, DisplayOptions};
use crate::color;
use crate::config::{Config, ProcessorConfig, find_registry_entry, registry_entries};
use crate::deps_cache::DepsCache;
use crate::errors;
use crate::file_index::FileIndex;
use crate::graph::BuildGraph;
use crate::object_store::{ObjectStore, ObjectStoreOptions};
use crate::processor::{Discovery, Processor, ProcessorMap};
use crate::remote_cache;
use crate::tool_lock;
use anyhow::{Context as _, Result};
use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Phase timing data collected during graph building.
pub type PhaseTimings = Vec<(String, Duration)>;

/// What graph construction produced.
struct GraphBuild {
    graph: BuildGraph,
    phase_timings: PhaseTimings,
    /// The dependency analysis, when the mode ran analyzers. A build hands
    /// it to the executor to re-analyze sources regenerated mid-build;
    /// everyone else drops it.
    analysis: Option<crate::analyzers::Analysis>,
}

/// Controls which graph-building variant to use.
#[derive(Clone, Copy, PartialEq, Eq)]
enum GraphBuildMode {
    /// Full build: discover products, run analyzers, resolve dependencies
    Normal,
    /// Clean: only discover products to find output files (skip expensive analysis)
    ForClean,
    /// Repair (`smart remove-no-file-processors`): discover products like a
    /// normal build, but tolerate `src_dirs` entries that don't exist. The
    /// command exists to delete exactly those stanzas, so it cannot refuse to
    /// run because of them.
    ForRepair,
}

/// Return the keys of a `HashMap` sorted alphabetically.
fn sorted_keys<V>(map: &HashMap<String, V>) -> Vec<&String> {
    let mut keys: Vec<&String> = map.keys().collect();
    keys.sort();
    keys
}

/// Check if phases debug is enabled.
fn phases_debug() -> bool {
    crate::runtime_flags::phases_debug()
}

/// A named point in the build pipeline at which `print_graph_stats` can
/// snapshot the graph. Each variant maps to a specific call site; adding a
/// new snapshot point means adding a variant here and a `print_graph_stats`
/// call at the site — the type checker keeps the two in sync.
///
/// The variant name (`SNAPSHOT_NAME` in `SCREAMING_SNAKE_CASE`) is printed
/// verbatim next to the counts so the user can grep for a specific point
/// or feed the symbolic name back into future CLI flags (e.g. stop-at).
#[derive(Debug, Clone, Copy)]
pub enum GraphSnapshot {
    /// Before any phase runs — empty graph.
    Start,
    /// After product discovery.
    AfterDiscover,
    /// After analyzer-driven dependency scanning.
    AfterAddDependencies,
    /// After tool-version hashes are folded into config hashes.
    AfterApplyToolHashes,
    /// After graph-wide dependency edge resolution.
    AfterResolve,
    /// After pre-build classify (skip/restore/build decision).
    AfterClassify,
    /// After the build has finished executing all products.
    AfterExecute,
}

impl GraphSnapshot {
    /// Symbolic name shown in graph-stats output. Matches the enum variant
    /// in `SCREAMING_SNAKE_CASE`.
    const fn name(self) -> &'static str {
        match self {
            Self::Start => "START",
            Self::AfterDiscover => "AFTER_DISCOVER",
            Self::AfterAddDependencies => "AFTER_ADD_DEPENDENCIES",
            Self::AfterApplyToolHashes => "AFTER_APPLY_TOOL_HASHES",
            Self::AfterResolve => "AFTER_RESOLVE",
            Self::AfterClassify => "AFTER_CLASSIFY",
            Self::AfterExecute => "AFTER_EXECUTE",
        }
    }
}

/// Emit a one-line snapshot of the graph's size at a named point.
/// No-op unless `--graph-stats` is set. Output goes to stderr so it can't
/// corrupt stdout piping (e.g., `rsconstruct graph show --format=dot`).
///
/// Three counters per snapshot:
///   - `products`: number of products in the graph.
///   - `edges`: producer→consumer edges (one product's output is another's input).
///   - `inputs`: total input-file references across all products. Headers added
///     by analyzers don't form `edges` (they aren't outputs of any product),
///     but they DO grow `inputs` — diff `inputs` between `AFTER_DISCOVER` and
///     `AFTER_ADD_DEPENDENCIES` to see how much the analyzer phase contributed.
pub fn print_graph_stats(snapshot: GraphSnapshot, graph: &BuildGraph) {
    if !crate::runtime_flags::graph_stats() {
        return;
    }
    let products = graph.products().len();
    // Cheap — each product's dependency list is a Vec already held in memory.
    let edges: usize = graph
        .products()
        .iter()
        .map(|p| graph.get_dependencies(p.id).len())
        .sum();
    let inputs: usize = graph.products().iter().map(|p| p.inputs.len()).sum();
    eprintln!(
        "[graph-stats] {:<24}  products={}  edges={}  inputs={}",
        snapshot.name(),
        products,
        edges,
        inputs,
    );
}

/// Labels for the four product states used by `dry_run` and status.
struct ProductStatusLabels<'a> {
    current: (Cow<'a, str>, &'static str),
    restorable: (Cow<'a, str>, &'static str),
    stale: (Cow<'a, str>, &'static str),
    new: (Cow<'a, str>, &'static str),
}

/// Options for `print_product_status`.
struct StatusPrintOptions<'a> {
    labels: &'a ProductStatusLabels<'a>,
    explain: bool,
    display_opts: DisplayOptions,
    verbose: bool,
    all_processor_names: &'a [&'a str],
}

/// Create a processor from a type name and TOML config value.
/// Returns None when the name is not a registered processor.
pub fn create_processor_for_instance(
    type_name: &str,
    config_toml: &toml::Value,
) -> anyhow::Result<Option<Box<dyn Processor>>> {
    if let Some(entry) = find_registry_entry(type_name) {
        let mut resolved = config_toml.clone();
        let mut prov = crate::config::ProvenanceMap::new();
        crate::registries::apply_all_defaults(&entry.pname(), &mut resolved, &mut prov);
        return (entry.create)(&resolved).map(Some);
    }
    Ok(None)
}

/// A stanza that cannot be turned into a processor or an analyzer: a config
/// error naming the stanza. The value of a field is only checked when its
/// config type is built (`max_jobs = -1`), so this is where such a mistake
/// surfaces, and it is the user's file that is wrong, not the build.
///
/// `path` is the stanza's key path in the config files, which is what gives
/// the message the file and line of the stanza's header.
fn stanza_error(
    config: &crate::config::Config,
    path: &[String],
    section: &str,
    error: &anyhow::Error,
) -> anyhow::Error {
    let location = config
        .sources
        .locate(path)
        .map_or_else(String::new, |location| format!("{location}: "));
    crate::exit_code::config_error(format!(
        "Invalid config:\n  {location}[{section}]: {error:#}"
    ))
}

/// The key path of a processor instance's stanza: `processor`, its type,
/// its name, and its instance segment when it has one.
fn processor_stanza_path(instance_name: &str) -> Vec<String> {
    let Some(parsed) = crate::registries::parse_name(instance_name) else {
        return Vec::new();
    };
    let mut path = vec![
        "processor".to_string(),
        parsed.processor_type.as_str().to_string(),
        parsed.short.to_string(),
    ];
    path.extend(parsed.instance.map(str::to_string));
    path
}

/// The key path of an analyzer instance's stanza: `analyzer`, its type, and
/// its instance segment when it has one.
fn analyzer_stanza_path(inst: &crate::config::AnalyzerInstance) -> Vec<String> {
    let mut path = vec!["analyzer".to_string(), inst.type_name.clone()];
    path.extend(
        inst.instance_name
            .strip_prefix(inst.type_name.as_str())
            .and_then(|rest| rest.strip_prefix('.'))
            .map(str::to_string),
    );
    path
}

/// Create a processor for every declared instance of `config`.
pub fn create_processors_of(config: &crate::config::Config) -> Result<ProcessorMap> {
    let mut processors: ProcessorMap = HashMap::new();
    for inst in &config.processor.instances {
        match create_processor_for_instance(&inst.pname, &inst.config_toml) {
            Ok(Some(proc)) => {
                processors.insert(inst.instance_name.clone(), proc);
            }
            Ok(None) => {
                anyhow::bail!("Unknown processor type: '{}'", inst.pname);
            }
            Err(e) => {
                return Err(stanza_error(
                    config,
                    &processor_stanza_path(&inst.instance_name),
                    &inst.instance_name,
                    &e,
                ));
            }
        }
    }
    Ok(processors)
}

/// Create an analyzer for every declared instance of `config`, disabled
/// ones included: whether one runs is the caller's business.
pub fn create_analyzers_of(
    config: &crate::config::Config,
    verbose: bool,
) -> Result<Vec<(String, Box<dyn DepAnalyzer>)>> {
    let mut analyzers = Vec::new();
    for inst in &config.analyzer.instances {
        let plugin = crate::registries::find_analyzer_plugin(&inst.type_name)
            .ok_or_else(|| anyhow::anyhow!("Unknown analyzer type '{}'", inst.type_name))?;
        let analyzer = (plugin.create)(&inst.config_toml, verbose).map_err(|e| {
            stanza_error(
                config,
                &analyzer_stanza_path(inst),
                &format!("analyzer.{}", inst.instance_name),
                &e,
            )
        })?;
        analyzers.push((inst.instance_name.clone(), analyzer));
    }
    Ok(analyzers)
}

/// Create all builtin processors with default configs.
pub fn create_all_default_processors() -> Result<ProcessorMap> {
    let mut processors: ProcessorMap = HashMap::new();
    for entry in registry_entries() {
        let pname = entry.pname();
        let processor = create_default_processor(entry)?;
        processors.insert(pname, processor);
    }
    Ok(processors)
}

/// The tools the processor `entry` registers runs at its default config,
/// as `tool list` counts them: a config can add more (pandoc's
/// `pdf_engine`).
pub fn default_processor_tools(
    entry: &crate::registries::processor::ProcessorPlugin,
) -> Result<Vec<String>> {
    let mut tools = create_default_processor(entry)?.required_tools();
    tools.sort();
    tools.dedup();
    Ok(tools)
}

/// Create the processor `entry` registers, at its default config.
fn create_default_processor(
    entry: &crate::registries::processor::ProcessorPlugin,
) -> Result<Box<dyn crate::processor::Processor>> {
    let pname = entry.pname();
    let mut empty_toml = toml::Value::Table(toml::map::Map::new());
    let mut provenance = crate::config::ProvenanceMap::new();
    crate::registries::apply_all_defaults(&pname, &mut empty_toml, &mut provenance);
    (entry.create)(&empty_toml)
        .with_context(|| format!("Failed to create processor '{pname}' with default config"))
}

pub struct Builder {
    object_store: ObjectStore,
    config: Config,
    file_index: FileIndex,
}

impl Builder {
    /// Apply config-derived settings to the `BuildContext`. Called by the
    /// constructors so every path gets it — watch mode used to miss this
    /// when it was a separate "remember to call after `new()`" step, silently
    /// ignoring `cache.mtime_check` under `rsconstruct watch`. The config
    /// value is applied in both directions on every call, so flipping
    /// `cache.mtime_check` while `rsconstruct watch` runs takes effect on
    /// the next rebuild; the context keeps the CLI `--no-mtime-cache`
    /// override separately, so it wins regardless of ordering.
    fn apply_config_to_context(&self, ctx: &crate::build_context::BuildContext) {
        ctx.set_mtime_check_from_config(
            self.config.cache.mtime_check == crate::config::MtimeCheck::On,
        );
        ctx.set_webcache_ttl_secs(self.config.cache.webcache_ttl_secs);
    }

    pub fn new(ctx: &crate::build_context::BuildContext) -> Result<Self> {
        Self::new_with_overrides(ctx, &[], &[])
    }

    /// Construct a Builder, applying CLI `--iset`/`--pset` config overrides
    /// after the rsconstruct.toml is loaded.
    pub fn new_with_overrides(
        ctx: &crate::build_context::BuildContext,
        iset: &[String],
        pset: &[String],
    ) -> Result<Self> {
        Config::require_config()?;
        let mut config = Config::load()?;
        config.apply_overrides(iset, pset)?;

        // Resolve auto restore method based on environment
        let restore_method = config.cache.restore_method.resolve();

        // (Compression with hardlink restore is rejected by `Config::load`.)

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
        })?;
        let (exclude_roots, force_dirs) = config.file_index_walk_dirs();
        let force_refs: Vec<&str> = force_dirs.iter().map(String::as_str).collect();
        let file_index = FileIndex::build_with_force_dirs(
            &force_refs,
            &exclude_roots,
            config.build.warn_symlinks,
        )?;

        let builder = Self {
            object_store,
            config,
            file_index,
        };
        builder.apply_config_to_context(ctx);
        Ok(builder)
    }

    /// Create processors from declared instances in the config.
    /// Only processors declared in `[processor.TYPE]` or `[processor.TYPE.NAME]` are created.
    pub fn create_processors(&self) -> Result<ProcessorMap> {
        create_processors_of(&self.config)
    }

    /// Detect and display config changes for each processor.
    /// Shows colored diffs when processor configuration has changed since last build.
    fn detect_config_changes(&self, processors: &ProcessorMap, show_all: bool) {
        // JSON/quiet mode suppresses the *printing* only — the stored config
        // baseline must still advance, or a CI running `--json` leaves the
        // next human build diffing against a months-old baseline.
        let print = !crate::json_output::is_json_mode() && !crate::runtime_flags::quiet();

        for name in sorted_keys(processors) {
            let processor = processors.get(name).expect(errors::PROCESSOR_NOT_IN_MAP);

            // Skip processors that don't provide config JSON
            let Some(config_json) = processor.config_json() else {
                continue;
            };

            // Keep only checksum-affecting fields for change detection (unless show_all).
            // Each processor declares which config fields affect its output;
            // changes to other fields (src_dirs, batch, max_jobs, etc.)
            // should not trigger config change detection by default.
            let config_json = if show_all {
                config_json
            } else {
                Self::filter_checksum_fields(name, &config_json)
            };

            // Store the config and check if it changed. A failed store is
            // reported: dropped silently, the next build would diff against
            // a stale baseline again and nobody would know why.
            let stored = match self.object_store.store_processor_config(name, &config_json) {
                Ok(stored) => stored,
                Err(e) => {
                    crate::output::warn(&format!(
                        "could not record the config of [{name}] for change detection: {e:#}"
                    ));
                    None
                }
            };
            if let Some(old_json) = stored
                && print
            {
                // Config changed - show diff
                if let Some(diff) = ObjectStore::diff_configs(&old_json, &config_json) {
                    println!(
                        "{}",
                        color::yellow(&format!("Config changed for [{name}]:"))
                    );
                    println!("{diff}");
                }
            }
        }
    }

    /// Filter a config JSON string to only include checksum-affecting fields.
    /// Uses the processor's `checksum_fields()` declaration to determine which
    /// fields matter for build output.
    fn filter_checksum_fields(processor_name: &str, json: &str) -> String {
        let checksum_fields = ProcessorConfig::checksum_fields_for(processor_name);
        let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
            return json.to_string();
        };
        let Some(obj) = value.as_object() else {
            return json.to_string();
        };
        checksum_fields.map_or_else(
            || json.to_string(),
            |fields| {
                let filtered: serde_json::Map<String, serde_json::Value> = obj
                    .iter()
                    .filter(|(k, _)| fields.contains(&k.as_str()))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                serde_json::to_string(&filtered).unwrap_or_else(|_| json.to_string())
            },
        )
    }

    /// Create dependency analyzers for each declared instance in `[analyzer.*]`.
    /// Only analyzers that appear in the config and have `enabled = true` are
    /// instantiated. Setting `enabled = false` lets a user disable an analyzer
    /// without removing its `[analyzer.X]` section.
    pub(crate) fn create_analyzers(
        &self,
        verbose: bool,
    ) -> Result<HashMap<String, Box<dyn DepAnalyzer>>> {
        Ok(create_analyzers_of(&self.config, verbose)?
            .into_iter()
            .filter(|(_, analyzer)| analyzer.enabled())
            .collect())
    }

    /// Run all declared dependency analyzers on the graph.
    ///
    /// Three-stage output matching the build phase's shape:
    ///   1. Forward total — how many files would be scanned if nothing is cached.
    ///   2. Pre-scan classify breakdown — how many are expected to hit the cache
    ///      vs need rescanning, based on a cheap dry-run of the validity check
    ///      (mtime shortcut where possible, full read + hash when mtime changed).
    ///   3. Post-scan summary — what actually happened.
    ///
    /// The pre-scan classify pass is observable separately (a place the user
    /// can stop and inspect state). It does the same work the scan would do
    /// anyway, but reads the result instead of acting on it — and the
    /// in-memory checksum cache in `checksum.rs` dedupes work across the two
    /// passes, so nothing is read+hashed twice.
    ///
    /// Returns the [`Analysis`](crate::analyzers::Analysis) so a build can
    /// re-analyze generated sources once their producers ran; every other
    /// caller drops it (which also closes the deps cache).
    /// `file_index` is the post-discovery index, declared outputs included
    /// (see `discover_products`): analyzers resolve references against the
    /// tree as the build will leave it, so a reference to a file another
    /// product generates is a dependency even before that file exists.
    fn run_analyzers(
        &self,
        ctx: &crate::build_context::BuildContext,
        graph: &mut BuildGraph,
        file_index: FileIndex,
        verbose: bool,
    ) -> Result<crate::analyzers::Analysis> {
        let analyzers = self.create_analyzers(verbose)?;

        // Only run analyzers that auto-detect relevant files in the project.
        let active_analyzers: Vec<(String, Box<dyn DepAnalyzer>)> = analyzers
            .into_iter()
            .filter(|(_, analyzer)| analyzer.auto_detect(&file_index))
            .collect();
        let mut analysis =
            crate::analyzers::Analysis::new(ctx, active_analyzers, DepsCache::open()?, file_index)?;

        if analysis.is_empty() {
            return Ok(analysis);
        }

        // Per-product reference count: every matching product becomes one
        // reference, even when many products share the same source. Used
        // to drive the progress bar (which ticks once per product) and to
        // surface fan-out in the user-facing total.
        let product_refs = analysis.count_matches(graph);

        // Unique source count: dedup (analyzer, source) pairs. This matches
        // what the cache and the scanner actually see — one cache key and one
        // file read per pair, regardless of how many products reference it.
        let unique_pairs = analysis.unique_sources(graph);
        let unique_count = unique_pairs.len();

        let suppress = crate::json_output::is_json_mode() || crate::runtime_flags::quiet();

        // Stage 1: announce the work in unique-source units, with the fan-out
        // shown in parentheses when products outnumber unique sources.
        if unique_count > 0 && !suppress {
            if product_refs > unique_count {
                println!(
                    "[deps] {unique_count} files to check (consumed by {product_refs} products)",
                );
            } else {
                println!("[deps] {unique_count} files to check");
            }
        }

        // Stage 2: the scan. There is no separate prediction pass: one used
        // to validate every cached entry (the source and each dependency)
        // only to print a "to rescan" count, then the scan validated them
        // all again, doubling this phase. The summary below reports the
        // same split from the scan itself.
        //
        // Progress bar still ticks per-product since
        // `analyze_graph` ticks once per consuming product (so the bar
        // matches what users expect from the build graph).
        let hidden = verbose || crate::json_output::is_json_mode() || crate::runtime_flags::quiet();
        let pb = crate::progress::create_bar(product_refs as u64, hidden);
        analysis.analyze_graph(ctx, graph, &pb)?;
        pb.finish_and_clear();

        let stats = analysis.stats();
        if unique_count > 0 && !suppress {
            println!(
                "[deps] summary: {} rescanned ({} cache hits: {} mtime, {} checksum)",
                stats.misses, stats.hits, stats.mtime_hits, stats.content_hits,
            );
        }

        Ok(analysis)
    }

    /// The complete graph built in `mode`, quietly.
    fn complete_graph(
        &self,
        ctx: &crate::build_context::BuildContext,
        processors: &ProcessorMap,
        mode: GraphBuildMode,
    ) -> Result<BuildGraph> {
        let built =
            self.build_graph_with_processors_impl(ctx, processors, mode, BuildPhase::Build, false)?;
        Ok(built.graph)
    }

    /// Build the dependency graph using provided processors
    fn build_graph_with_processors(
        &self,
        ctx: &crate::build_context::BuildContext,
        processors: &ProcessorMap,
    ) -> Result<BuildGraph> {
        self.complete_graph(ctx, processors, GraphBuildMode::Normal)
    }

    /// Like `build_graph_with_processors`, but tolerant of `src_dirs`
    /// entries that don't exist (see `GraphBuildMode::ForRepair`).
    fn build_graph_for_repair_with_processors(
        &self,
        ctx: &crate::build_context::BuildContext,
        processors: &ProcessorMap,
    ) -> Result<BuildGraph> {
        self.complete_graph(ctx, processors, GraphBuildMode::ForRepair)
    }

    /// Build the dependency graph for clean (skip expensive dependency scanning)
    fn build_graph_for_clean_with_processors(
        &self,
        ctx: &crate::build_context::BuildContext,
        processors: &ProcessorMap,
    ) -> Result<BuildGraph> {
        self.complete_graph(ctx, processors, GraphBuildMode::ForClean)
    }

    /// Return the set of processor type names whose files are detected in the project.
    /// Uses default configs for all builtin processors to check `auto_detect`.
    pub fn detected_processors(&self) -> Result<std::collections::HashSet<String>> {
        let processors = create_all_default_processors()?;
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
        let processors = create_all_default_processors()?;
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
    ///
    /// Processors with `enabled = false` are excluded: discovery skips them, so they
    /// produce 0 products by definition. Reporting them would flag a deliberately
    /// disabled stanza as dead config and invite its removal.
    pub fn no_file_processors(
        &self,
        ctx: &crate::build_context::BuildContext,
    ) -> Result<Vec<String>> {
        let processors = self.create_processors()?;
        let graph = self.build_graph_for_repair_with_processors(ctx, &processors)?;

        let mut has_products: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for product in graph.products() {
            has_products.insert(product.processor.as_str());
        }

        let mut empty: Vec<String> = sorted_keys(&processors)
            .into_iter()
            .filter(|name| processors[name.as_str()].scan_config().enabled)
            .filter(|name| !has_products.contains(name.as_str()))
            .cloned()
            .collect();
        empty.sort();
        Ok(empty)
    }

    /// Check whether a processor should run. In the instance-based model,
    /// every declared processor is active unless its stanza says
    /// `enabled = false` — the one way to keep a stanza out of the build.
    fn is_processor_active(processor: &dyn Processor) -> bool {
        processor.scan_config().enabled
    }

    /// Product discovery, routing generated files to the processors that
    /// consume them (docs/src/internal/output-routing.md).
    ///
    /// Round 0 runs every processor over the files on disk. Each later round
    /// takes the outputs the previous round declared that are new to the
    /// index ("virtual files": they exist only once their producer has run)
    /// and hands them on:
    ///
    /// - a [`Discovery::PerFile`] processor discovers over an index of only
    ///   those new files, so it creates products for exactly the ones its
    ///   scan settings accept and never rescans the rest;
    /// - any other processor rediscovers over the whole index, as every
    ///   processor did under the old fixed-point loop.
    ///
    /// Connections are inferred, never configured: a processor consumes a
    /// generated file exactly when its scan settings accept the path, the
    /// same rule as for files on disk. Discovery ends when a round declares
    /// no new file.
    ///
    /// Returns the file index with every declared output added: the view
    /// of the tree as it will be once the build ran, which is what
    /// dependency analysis resolves references against.
    fn discover_products(
        &self,
        graph: &mut BuildGraph,
        processors: &ProcessorMap,
        active: &[impl AsRef<str>],
        mode: GraphBuildMode,
    ) -> Result<FileIndex> {
        let for_clean = mode == GraphBuildMode::ForClean;
        let mut file_index = self.file_index.clone();
        let debug = phases_debug();
        let max_passes = self.config.build.max_discovery_passes;
        // The files the previous round declared; None in round 0.
        let mut routed: Option<FileIndex> = None;

        for pass in 0..=max_passes {
            let before = graph.products().len();
            for name in active {
                let name = name.as_ref();
                let processor = &processors[name];
                if !processor.scan_config().enabled {
                    continue;
                }
                let view = match (&routed, processor.discovery()) {
                    (Some(new_files), Discovery::PerFile) => new_files,
                    _ => &file_index,
                };
                if for_clean {
                    processor.discover_for_clean(graph, view, name)?;
                } else {
                    processor.discover(graph, view, name)?;
                }
            }
            let after = graph.products().len();
            if after == before {
                break;
            }
            if pass == max_passes {
                // `max_passes` passes may add products; this one more only
                // confirms that the files the last of them declared feed
                // nothing. It found products instead, so the chain is
                // longer than the cap and the graph would be silently
                // truncated. Fail loudly and name the culprits.
                let mut culprits: Vec<&str> = graph.products()[before..after]
                    .iter()
                    .map(|p| p.processor.as_str())
                    .collect();
                culprits.sort_unstable();
                culprits.dedup();
                return Err(crate::exit_code::RsconstructError::new(
                    crate::exit_code::RsconstructExitCode::GraphError,
                    format!(
                        "discovery did not converge after {max_passes} passes; still adding products: {} (raise [build] max_discovery_passes if this chain is intentional)",
                        culprits.join(", ")
                    ),
                ).into());
            }
            let outputs: Vec<PathBuf> = graph.products()[before..after]
                .iter()
                .flat_map(|p| p.outputs.iter().cloned())
                .collect();
            let added = file_index.add_virtual_files(&outputs);
            if added.is_empty() {
                break;
            }
            if debug {
                eprintln!(
                    "{}",
                    color::dim(&format!(
                        "    discover pass {}: {} new products, {} virtual files added",
                        pass + 1,
                        after - before,
                        added.len()
                    ))
                );
            }
            routed = Some(FileIndex::from_paths(added));
        }

        // After the fixed-point loop has settled, check every src_dirs and
        // src_files entry is backed by something: a real directory or file on
        // disk, or virtual files injected by an upstream processor's declared
        // outputs (paths that only exist once that processor has run).
        //
        // A missing entry is a config error. src_dirs and src_files are claims
        // about where this project keeps its sources; an entry naming a path
        // that is not there is a typo, a stale stanza from a path that moved,
        // or a copy of another repo's config — and in every case a processor
        // that silently checks nothing while the build stays green.
        //
        // `[build] allow_missing_src_dirs = true` restores the old skip for
        // directories only, reported under --phases. A missing src_files
        // entry is never forgiven: a directory can at least plausibly be
        // empty, a named file that is not there is simply wrong.
        //
        // Clean and repair tolerate missing entries too: `clean` must be able
        // to remove the outputs of a build whose sources have since gone, and
        // `smart remove-no-file-processors` exists to delete the very stanzas
        // this check rejects.
        let mut missing_dirs: Vec<String> = Vec::new();
        let mut missing_files: Vec<String> = Vec::new();
        for name in active {
            let name = name.as_ref();
            let scan = processors[name].scan_config();
            if !scan.enabled {
                continue;
            }
            for dir in scan.src_dirs() {
                if dir.is_empty() || dir == "." {
                    continue;
                }
                if std::path::Path::new(dir).is_dir() {
                    continue;
                }
                let prefix = std::path::PathBuf::from(dir);
                let covered_by_virtual = file_index.files().iter().any(|f| f.starts_with(&prefix));
                if !covered_by_virtual {
                    missing_dirs.push(format!(
                        "  [{name}] src_dirs entry '{dir}' does not exist or is not a directory"
                    ));
                }
            }
            for file in scan.src_files() {
                if std::path::Path::new(file).is_file() {
                    continue;
                }
                let path = std::path::PathBuf::from(file);
                let covered_by_virtual = file_index.files().contains(&path);
                if !covered_by_virtual {
                    missing_files.push(format!(
                        "  [{name}] src_files entry '{file}' does not exist or is not a file"
                    ));
                }
            }
        }
        let tolerant = mode != GraphBuildMode::Normal;
        if tolerant || self.config.build.allow_missing_src_dirs == crate::config::MissingEntry::Skip
        {
            if debug {
                for entry in &missing_dirs {
                    eprintln!(
                        "{}",
                        color::dim(&format!("  {} (skipped)", entry.trim_start()))
                    );
                }
            }
            missing_dirs.clear();
        }
        if tolerant {
            if debug {
                for entry in &missing_files {
                    eprintln!(
                        "{}",
                        color::dim(&format!("  {} (skipped)", entry.trim_start()))
                    );
                }
            }
            missing_files.clear();
        }
        // The explicit processor's literal `inputs` are the same kind of
        // claim as src_files, with the same proviso: an entry may name a
        // file an upstream processor only produces during the build, so it
        // cannot be checked round by round — only here, against the settled
        // index. The products carry every literal entry as declared
        // (`processor.explicit.generic` resolves nothing away), so a product
        // input that is neither on disk nor a declared output is a typo or
        // a stale stanza, and the command would run against a file that is
        // not there.
        let mut missing_inputs: Vec<String> = Vec::new();
        if !tolerant {
            let explicit_prefix = format!("{}.", crate::processor::names::EXPLICIT);
            for product in graph.products() {
                if !product.processor.starts_with(&explicit_prefix) {
                    continue;
                }
                for input in &product.inputs {
                    if input.is_file() || file_index.contains(input) {
                        continue;
                    }
                    missing_inputs.push(format!(
                        "  [{}] inputs entry '{}' does not exist and no processor declares it as an output",
                        product.processor,
                        input.display()
                    ));
                }
            }
            // A literal `dep_inputs` entry is the same kind of claim again:
            // a file on disk, or one an upstream processor declares as its
            // output (a spellcheck's `dep_inputs` naming the dictionary a
            // generator compiles, which is what orders the two). Processors
            // put the entry on their products unchecked, so it is checked
            // here, for every processor that has a product to read it.
            // Globs are not claims about one file and match what they match.
            let with_products: std::collections::HashSet<&str> = graph
                .products()
                .iter()
                .map(|p| p.processor.as_str())
                .collect();
            for name in active {
                let name = name.as_ref();
                let scan = processors[name].scan_config();
                if !scan.enabled || !with_products.contains(name) {
                    continue;
                }
                for dep in &scan.dep_inputs {
                    if dep.contains('*') || dep.contains('?') || dep.contains('[') {
                        continue;
                    }
                    let path = std::path::Path::new(dep);
                    if path.exists() || file_index.contains(path) {
                        continue;
                    }
                    missing_inputs.push(format!(
                        "  [{name}] dep_inputs file not found: {dep} (not on disk, and no processor declares it as an output)"
                    ));
                }
            }
            // A stanza whose inputs resolved to nothing waits round by
            // round for an upstream output to appear (see its `discover`).
            // Still without a product now, nothing will ever feed it: a
            // mistyped input_globs made the stanza vanish from the build
            // with exit 0 and its output never produced.
            for name in active {
                let name = name.as_ref();
                if name != crate::processor::names::EXPLICIT && !name.starts_with(&explicit_prefix)
                {
                    continue;
                }
                let processor = &processors[name];
                if !processor.scan_config().enabled || !processor.auto_detect(&file_index) {
                    continue;
                }
                if !graph.products().iter().any(|p| p.processor == name) {
                    missing_inputs.push(format!(
                        "  [{name}] no input: its inputs and input_globs resolve to no file, so \
                         the stanza would never run"
                    ));
                }
            }
        }
        if missing_dirs.is_empty() && missing_files.is_empty() && missing_inputs.is_empty() {
            return Ok(file_index);
        }
        let mut missing = missing_dirs;
        missing.extend(missing_files);
        missing.extend(missing_inputs);
        Err(crate::exit_code::config_error(format!(
            "Invalid config:\n{}\nEvery src_dirs entry must name a directory that exists, and every \
             src_files entry, every explicit inputs entry and every dep_inputs entry a file that \
             exists (or a path an upstream processor declares as its output), and every explicit \
             stanza must have at \
             least one input — fix the path or remove the entry; \
             [build] allow_missing_src_dirs = true skips absent directories as before, never \
             absent files",
            missing.join("\n")
        )))
    }

    /// Instance names whose config sets `output_depends_on_input_name`.
    /// Read from the declared config, not from the processors: the field is
    /// the framework's, applied to the products in the graph.
    fn processors_keyed_by_input_paths(&self) -> std::collections::HashSet<String> {
        self.config
            .processor
            .instances
            .iter()
            .filter(|inst| {
                inst.config_toml
                    .get(crate::config::OUTPUT_DEPENDS_ON_INPUT_NAME)
                    .and_then(toml::Value::as_bool)
                    == Some(true)
            })
            .map(|inst| inst.instance_name.clone())
            .collect()
    }

    /// Instance names whose config sets `batch_by_dir`. Their products
    /// share the files of their directory as inputs
    /// (`BuildGraph::share_dir_inputs`).
    fn processors_batched_by_dir(&self) -> std::collections::HashSet<String> {
        self.config
            .processor
            .instances
            .iter()
            .filter(|inst| {
                inst.config_toml
                    .get(crate::config::BATCH_BY_DIR)
                    .and_then(toml::Value::as_bool)
                    == Some(true)
            })
            .map(|inst| inst.instance_name.clone())
            .collect()
    }

    /// Key the products of every processor that depends on the project's
    /// Python environment by its fingerprint (see `python_env`). Nothing
    /// to do without a `./.venv`.
    fn apply_python_env(&self, graph: &mut BuildGraph, processors: &ProcessorMap) -> Result<()> {
        let cwd = std::env::current_dir().context("Failed to read the current directory")?;
        let venv = cwd.join(crate::python_env::VENV_DIR);
        let Some(fingerprint) = crate::python_env::fingerprint(&venv) else {
            return Ok(());
        };
        let explicit: std::collections::HashMap<&str, bool> = self
            .config
            .processor
            .instances
            .iter()
            .filter_map(|inst| {
                inst.config_toml
                    .get(crate::config::HASH_PYTHON_ENV)
                    .and_then(toml::Value::as_bool)
                    .map(|v| (inst.instance_name.as_str(), v))
            })
            .collect();
        let keyed = crate::python_env::processors_keyed_by_python_env(
            processors,
            &|name| tools::instance_enabled(processors, name),
            &|name| explicit.get(name).copied(),
            &venv,
        );
        graph.apply_python_env(&keyed, &fingerprint);
        Ok(())
    }

    /// Build the dependency graph using provided processors. Every active
    /// processor takes part: `-p`/`-x` select from the finished graph (see
    /// `select_processors`), since a selection needs its producers.
    fn build_graph_with_processors_impl(
        &self,
        ctx: &crate::build_context::BuildContext,
        processors: &ProcessorMap,
        mode: GraphBuildMode,
        stop_after: BuildPhase,
        verbose: bool,
    ) -> Result<GraphBuild> {
        if phases_debug() {
            eprintln!("{}", color::bold("Phase: Building dependency graph..."));
        }
        let mut graph = BuildGraph::new();
        let mut phase_timings = PhaseTimings::new();
        let mut analysis = None;
        print_graph_stats(GraphSnapshot::Start, &graph);

        // Collect which processors should run
        let active_processors: Vec<&String> = sorted_keys(processors)
            .into_iter()
            .filter(|name| Self::is_processor_active(processors[*name].as_ref()))
            .collect();

        // Phase 1: Discover products
        if phases_debug() {
            crate::output::diagnostic(&color::dim("  Phase: discover"));
        }
        let t = Instant::now();
        let file_index =
            self.discover_products(&mut graph, processors, &active_processors, mode)?;
        graph.key_by_input_paths(&self.processors_keyed_by_input_paths());
        graph.share_dir_inputs(&self.processors_batched_by_dir());
        phase_timings.push(("discover".to_string(), t.elapsed()));
        print_graph_stats(GraphSnapshot::AfterDiscover, &graph);

        if stop_after == BuildPhase::Discover {
            return Ok(GraphBuild {
                graph,
                phase_timings,
                analysis,
            });
        }

        // Phase 2: Run dependency analyzers (only for regular builds, not clean)
        if mode == GraphBuildMode::Normal {
            if phases_debug() {
                crate::output::diagnostic(&color::dim("  Phase: add_dependencies"));
            }
            let t = Instant::now();
            analysis = Some(self.run_analyzers(ctx, &mut graph, file_index, verbose)?);
            phase_timings.push(("add_dependencies".to_string(), t.elapsed()));
            print_graph_stats(GraphSnapshot::AfterAddDependencies, &graph);
        }

        if stop_after == BuildPhase::AddDependencies {
            return Ok(GraphBuild {
                graph,
                phase_timings,
                analysis,
            });
        }

        // Phase 3: Apply tool version hashes
        if phases_debug() {
            crate::output::diagnostic(&color::dim("  Phase: apply_tool_version_hashes"));
        }
        let t = Instant::now();
        if self.config.build.hash_tool_versions {
            let tool_hashes = tool_lock::processor_tool_hashes(processors, &|name| {
                tools::instance_enabled(processors, name)
            });
            if !tool_hashes.is_empty() {
                graph.apply_tool_version_hashes(&tool_hashes);
            }
        }
        phase_timings.push(("tool_version_hashes".to_string(), t.elapsed()));

        // Phase 3b: the project's Python environment, for the processors
        // whose results depend on the installed packages.
        let t = Instant::now();
        if self.config.build.hash_python_env {
            self.apply_python_env(&mut graph, processors)?;
        }
        phase_timings.push(("python_env".to_string(), t.elapsed()));
        print_graph_stats(GraphSnapshot::AfterApplyToolHashes, &graph);

        // Phase 4: Resolve dependencies
        if phases_debug() {
            crate::output::diagnostic(&color::dim("  Phase: resolve_dependencies"));
        }
        let t = Instant::now();
        graph.resolve_dependencies();
        phase_timings.push(("resolve".to_string(), t.elapsed()));
        print_graph_stats(GraphSnapshot::AfterResolve, &graph);

        // Phase 5: Validate graph
        let t = Instant::now();
        let validation_errors = graph.validate(&self.config.graph);
        if !validation_errors.is_empty() {
            anyhow::bail!("Graph validation failed:\n{}", validation_errors.join("\n"));
        }
        phase_timings.push(("validate".to_string(), t.elapsed()));

        // Note: BuildPhase::Resolve and BuildPhase::Build both complete the graph
        Ok(GraphBuild {
            graph,
            phase_timings,
            analysis,
        })
    }

    /// Build the dependency graph (creates processors internally)
    fn build_graph(&self, ctx: &crate::build_context::BuildContext) -> Result<BuildGraph> {
        let processors = self.create_processors()?;
        self.build_graph_with_processors(ctx, &processors)
    }

    /// Build the dependency graph for cache operations (public).
    pub fn build_graph_for_cache(
        &self,
        ctx: &crate::build_context::BuildContext,
    ) -> Result<BuildGraph> {
        self.build_graph(ctx)
    }

    /// Compute the set of valid descriptor keys from the current build graph.
    /// These must be the exact keys the executor stores descriptors under
    /// (`Product::descriptor_key` over the combined input checksum) — `cache
    /// stale` and `cache remove-stale` compare them against the keys
    /// reconstructed from on-disk descriptor paths.
    ///
    /// A key is only known when the product's inputs are on disk. An input
    /// that another product generates and that is absent (after `clean
    /// outputs`, say) hashes as missing, which is not the key the entry was
    /// stored under: the entry would be judged stale and removed although
    /// the next build restores from it. That state is an error here, not a
    /// guess.
    pub fn valid_cache_keys(
        &self,
        ctx: &crate::build_context::BuildContext,
    ) -> Result<std::collections::HashSet<String>> {
        let graph = self.build_graph_for_cache(ctx)?;
        for product in graph.products() {
            if let Some(absent) = product
                .inputs
                .iter()
                .find(|input| graph.path_owner(input).is_some() && !input.exists())
            {
                anyhow::bail!(
                    "cannot tell which cache entries are stale: {} is generated by another product and is not on disk, \
                     so the cache key of [{}] {} is unknown. Run a build first.",
                    absent.display(),
                    product.processor,
                    product.primary_input().display(),
                );
            }
        }
        graph
            .products()
            .iter()
            .map(|product| {
                let input_checksum =
                    crate::checksum::combined_input_checksum(ctx, &product.inputs)?;
                Ok(product.descriptor_key(&input_checksum))
            })
            .collect()
    }

    /// Get a reference to the object store.
    pub const fn object_store(&self) -> &ObjectStore {
        &self.object_store
    }

    /// The configured global output directory (default "out").
    pub fn output_dir(&self) -> &str {
        &self.config.build.output_dir
    }

    /// Return directories that should be watched for file changes.
    /// Derived from processor scan configs plus standard project files.
    pub fn watch_paths(&self) -> Vec<PathBuf> {
        let mut paths: Vec<PathBuf> = Vec::new();

        // Always watch rsconstruct.toml
        let config_path = PathBuf::from(crate::config::CONFIG_FILE);
        if config_path.exists() {
            paths.push(config_path);
        }

        // Add scan directories from all processor configs
        for dir in self.config.processor.src_dirs() {
            let full = PathBuf::from(&dir);
            if full.exists() {
                paths.push(full);
            }
        }

        // Processors with empty default_src_dirs scan the project root — watch common
        // top-level files/dirs that wouldn't be covered by src_dirs above.
        for name in &["pyproject.toml", "config"] {
            let p = PathBuf::from(name);
            if p.exists() {
                paths.push(p);
            }
        }

        paths.sort();
        paths.dedup();
        paths
    }
}
