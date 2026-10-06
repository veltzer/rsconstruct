//! Dependency analyzers for scanning source files and adding dependencies to the build graph.
//!
//! Analyzers are separate from processors - they run after product discovery to add
//! dependency information (like header files for C/C++ or imports for Python).

mod cpp;
mod icpp;
mod markdown;
pub mod python;
mod sass;
mod tera;

use crate::deps_cache::{AnalyzerId, ClassifyResult, DepsCache, DepsCacheStats};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processor::{format_command, run_command_capture};
use anyhow::{Context, Result};
use indicatif::ProgressBar;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Trait for dependency analyzers that scan source files and add dependencies to the graph.
///
/// Analyzers run after processors have discovered products. They scan source files
/// to find dependencies (like #include for C/C++ or import for Python) and add
/// them to the appropriate products in the graph.
///
/// Must be Sync + Send for potential parallel execution.
pub trait DepAnalyzer: Sync + Send {
    /// Human-readable description of what this analyzer does.
    fn description(&self) -> &str;

    /// Whether this analyzer is active. Default true; override to respect
    /// the `enabled` field on an analyzer's config struct.
    fn enabled(&self) -> bool {
        true
    }

    /// Auto-detect if this analyzer is relevant for the project.
    /// Called with the file index to check for relevant file types.
    fn auto_detect(&self, file_index: &FileIndex) -> bool;

    /// Return the source path this analyzer would scan for the given product,
    /// or None if the product is not relevant. Used by the shared progress bar
    /// in `run_analyzers` to compute an accurate total before scanning starts,
    /// and by `analyze_with_scanner` to filter products inside the analyze loop.
    fn match_product(&self, product: &Product) -> Option<PathBuf>;

    /// Count how many products in the graph this analyzer would scan.
    /// Default impl iterates over products and calls `match_product`; override
    /// only if a cheaper count is available.
    fn count_matches(&self, graph: &BuildGraph) -> usize {
        graph
            .products()
            .iter()
            .filter(|p| self.match_product(p).is_some())
            .count()
    }

    /// Return the set of source paths this analyzer would scan. Used by the
    /// pre-scan classify pass to predict cache-hit / rescan counts before any
    /// work runs. Default impl iterates over products and collects each
    /// `match_product` result.
    fn matching_sources(&self, graph: &BuildGraph) -> Vec<PathBuf> {
        graph
            .products()
            .iter()
            .filter_map(|p| self.match_product(p))
            .collect()
    }

    /// Scan one source file (a `match_product` result) for its dependencies.
    ///
    /// Report in `ScanResult::absent` every path the resolution probed and
    /// found missing before settling on its answer — the deps cache drops
    /// the entry once one of them appears. Iterating products, the deps
    /// cache and applying the result to the graph are the framework's job
    /// (see [`analyze_graph`]).
    fn scan(
        &self,
        ctx: &crate::build_context::BuildContext,
        source: &Path,
        file_index: &FileIndex,
    ) -> Result<ScanResult>;

    /// Rescan every source on every run instead of trusting the deps cache.
    /// Required for an analyzer whose `ScanResult` carries `hash_pieces`:
    /// those depend on filesystem state (glob results) the per-source cache
    /// cannot represent, so they are never cached.
    fn always_rescan(&self) -> bool {
        false
    }

    /// Everything besides file contents that decides what `scan` resolves:
    /// the analyzer's configuration and any search paths it resolves at run
    /// time (pkg-config, include-path commands). Hashed into the
    /// fingerprint every deps-cache entry is stored under, so a config
    /// change rescans instead of trusting lists resolved the old way.
    fn fingerprint_parts(&self, ctx: &crate::build_context::BuildContext) -> Result<Vec<String>>;

    /// Recompute the hash pieces this analyzer would contribute for `source`,
    /// without touching the build graph or the deps cache. Used by
    /// `analyzer show files <path> --hash-pieces` to surface the non-content
    /// state (resolved glob sets, embedded shell commands, etc.) that an
    /// analyzer mixes into a product's cache key.
    ///
    /// The default impl returns `None`, meaning "this analyzer does not
    /// contribute hash pieces" (most don't — only Tera does today). Override
    /// when the analyzer's `analyze` populates `ScanResult.hash_pieces`.
    fn scan_hash_pieces(
        &self,
        _ctx: &crate::build_context::BuildContext,
        _source: &Path,
    ) -> Result<Option<Vec<String>>> {
        Ok(None)
    }
}

/// Query pkg-config for include paths from the given packages.
/// Uses `pkg-config --cflags-only-I` and strips the `-I` prefix.
/// Returns an empty list if `packages` is empty.
///
/// Any failure is an error — pkg-config missing, an unknown package, a
/// non-zero exit. These used to be printed and skipped, which left the
/// analyzer resolving headers without the package's include directories:
/// those headers silently dropped out of every dependency list, so editing
/// one rebuilt nothing, and the build stayed green.
///
/// - `tag`: prefix for log messages (e.g., "cpp" or "icpp")
/// - `packages`: pkg-config package names to query
/// - `verbose`: whether to emit diagnostic messages to stderr
pub fn query_pkg_config_include_paths(
    ctx: &crate::build_context::BuildContext,
    tag: &str,
    packages: &[String],
    verbose: bool,
) -> Result<Vec<PathBuf>> {
    if packages.is_empty() {
        return Ok(Vec::new());
    }

    let mut cmd = Command::new("pkg-config");
    cmd.arg("--cflags-only-I");
    cmd.args(packages);

    if verbose {
        eprintln!("[{}] Querying pkg-config: {}", tag, format_command(&cmd));
    }

    let output = run_command_capture(ctx, &cmd)
        .with_context(|| format!("[{tag}] Failed to run {}", format_command(&cmd)))?;
    if !output.status.success() {
        anyhow::bail!(
            "[{}] {} failed ({}): {}",
            tag,
            format_command(&cmd),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    // A package with no -I flags (its headers live in a default directory)
    // is a valid answer, not a failure.
    let paths: Vec<PathBuf> = String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .filter_map(|flag| flag.strip_prefix("-I").map(PathBuf::from))
        .collect();

    if verbose && !paths.is_empty() {
        eprintln!(
            "[{}] Found {} include paths from pkg-config",
            tag,
            paths.len()
        );
    }

    Ok(paths)
}

/// Run each command in `commands` via `sh -c` and collect its stdout
/// (trimmed) as an include path.
///
/// Every command must succeed and print an existing directory; anything
/// else — an empty entry, a failure, empty output, a path that is not a
/// directory — is an error naming the command. These used to be skipped
/// with a warning (the last only under `-v`), with the same silent loss of
/// dependencies as a failed pkg-config query.
///
/// - `tag`: prefix for log messages (e.g., "cpp" or "icpp")
/// - `commands`: shell command strings to run
/// - `verbose`: whether to emit diagnostic messages to stderr
pub fn run_include_path_commands(
    ctx: &crate::build_context::BuildContext,
    tag: &str,
    commands: &[String],
    verbose: bool,
) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for cmd_str in commands {
        if cmd_str.trim().is_empty() {
            anyhow::bail!("[{tag}] include_path_commands has an empty entry");
        }

        // Run via shell to support shell syntax (command substitution, etc.)
        let mut cmd = Command::new("sh");
        cmd.arg("-c");
        cmd.arg(cmd_str);

        if verbose {
            eprintln!("[{tag}] Running include path command: sh -c '{cmd_str}'");
        }

        let output = run_command_capture(ctx, &cmd)
            .with_context(|| format!("[{tag}] Failed to run include path command '{cmd_str}'"))?;
        if !output.status.success() {
            anyhow::bail!(
                "[{}] Include path command '{}' failed ({}): {}",
                tag,
                cmd_str,
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }

        let path_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if path_str.is_empty() {
            anyhow::bail!("[{tag}] Include path command '{cmd_str}' printed nothing");
        }
        let path = PathBuf::from(&path_str);
        if !path.is_dir() {
            anyhow::bail!(
                "[{tag}] Include path command '{cmd_str}' printed '{path_str}', which is not a directory"
            );
        }
        if verbose {
            eprintln!(
                "[{}] Added include path from command: {}",
                tag,
                path.display()
            );
        }
        paths.push(path);
    }

    if verbose && !paths.is_empty() {
        eprintln!(
            "[{}] Found {} include paths from commands",
            tag,
            paths.len()
        );
    }

    Ok(paths)
}

/// Return the include paths cached in `cell`, resolving them with
/// `resolve` on first use. A failed resolution is returned, not cached:
/// analysis stops at the first error anyway (`Analysis::new` resolves them
/// through `fingerprint_parts` before any file is scanned).
pub fn cached_include_paths(
    cell: &std::sync::OnceLock<Vec<PathBuf>>,
    resolve: impl FnOnce() -> Result<Vec<PathBuf>>,
) -> Result<&[PathBuf]> {
    if let Some(paths) = cell.get() {
        return Ok(paths);
    }
    let paths = resolve()?;
    Ok(cell.get_or_init(|| paths))
}

/// Result of scanning a single source file: a list of dependency paths and
/// a list of structured pieces mixed into each affected product's `config_hash`.
///
/// `hash_pieces` is for analyzer state that must invalidate the cache key but
/// is *not* a file content (e.g. the sorted set of paths matching a glob
/// pattern, or the literal text of a shell command embedded in a template).
/// Each piece is a `kind:body` string; the order is determined by the analyzer
/// and must be stable across runs. The pieces are joined with `|` and mixed
/// into the existing `config_hash`, so adding/removing entries from the set or
/// rewording the command flips the key even when no individual input file's
/// content changed.
///
/// The pieces are also surfaced via `rsconstruct analyzer show files <path>
/// --hash-pieces` so users can see exactly what non-content state the analyzer
/// is tracking for a given source.
///
/// `absent` lists the paths resolution probed and found missing on the way
/// to `deps` (an earlier include directory, the `.py` candidate before the
/// package `__init__.py`). If one of them appears later it may shadow what
/// was found, so the deps cache treats the entry as stale.
#[derive(Debug, Default)]
pub struct ScanResult {
    pub deps: Vec<PathBuf>,
    pub hash_pieces: Vec<String>,
    pub absent: Vec<PathBuf>,
}

impl ScanResult {
    /// A result with dependencies and absent probes but no hash pieces —
    /// what every analyzer except the glob-aware ones returns.
    pub const fn deps(deps: Vec<PathBuf>, absent: Vec<PathBuf>) -> Self {
        Self {
            deps,
            hash_pieces: Vec::new(),
            absent,
        }
    }
}

/// The cache-key piece a scan contributes to every product it applies to,
/// or None when it has no hash pieces. Length-prefixed hash, not a '|' join:
/// pieces embed file paths, which can contain any separator.
fn joined_hash_pieces(result: &ScanResult) -> Option<String> {
    if result.hash_pieces.is_empty() {
        return None;
    }
    let parts: Vec<&str> = result.hash_pieces.iter().map(String::as_str).collect();
    Some(crate::checksum::hash_parts(&parts))
}

/// One declared, enabled analyzer instance and the deps-cache fingerprint
/// it scans under.
struct ActiveAnalyzer {
    iname: String,
    analyzer: Box<dyn DepAnalyzer>,
    fingerprint: String,
}

/// The dependency analysis of one graph: the active analyzers, in the order
/// they contribute, and the deps cache they share.
///
/// Built and run during graph construction ([`analyze_graph`]). A build
/// keeps it alive into execution: a file that some product *generates* is
/// analyzed at graph time in whatever state it is on disk — or not at all,
/// on a clean checkout — and that state is stale once its producer rebuilds
/// it. [`reanalyze`] redoes the analysis of every product that read such a
/// file right after the producer ran, so the product is checksummed,
/// ordered and cached against what it will actually read.
///
/// [`analyze_graph`]: Self::analyze_graph
/// [`reanalyze`]: Self::reanalyze
pub struct Analysis {
    /// Sorted by instance name. The order analyzers add inputs and key
    /// pieces is part of each product's cache key, so re-analysis must
    /// replay it exactly.
    analyzers: Vec<ActiveAnalyzer>,
    deps_cache: DepsCache,
    /// The project's files plus every declared output, so a reference to a
    /// file that another product will generate resolves to it.
    file_index: FileIndex,
}

impl Analysis {
    /// Fingerprint each analyzer (see `DepAnalyzer::fingerprint_parts`) and
    /// order them by instance name.
    pub fn new(
        ctx: &crate::build_context::BuildContext,
        analyzers: Vec<(String, Box<dyn DepAnalyzer>)>,
        deps_cache: DepsCache,
        file_index: FileIndex,
    ) -> Result<Self> {
        let mut active: Vec<ActiveAnalyzer> = analyzers
            .into_iter()
            .map(|(iname, analyzer)| {
                // This is where pkg-config and the include-path commands
                // run, so the context speaks of the analyzer's setup.
                let parts = analyzer
                    .fingerprint_parts(ctx)
                    .with_context(|| format!("Failed to set up analyzer '{iname}'"))?;
                let parts: Vec<&str> = parts.iter().map(String::as_str).collect();
                Ok(ActiveAnalyzer {
                    fingerprint: crate::checksum::hash_parts(&parts),
                    iname,
                    analyzer,
                })
            })
            .collect::<Result<_>>()?;
        active.sort_by(|a, b| a.iname.cmp(&b.iname));
        Ok(Self {
            analyzers: active,
            deps_cache,
            file_index,
        })
    }

    pub const fn is_empty(&self) -> bool {
        self.analyzers.is_empty()
    }

    /// Products matched, summed over analyzers: the progress-bar total.
    pub fn count_matches(&self, graph: &BuildGraph) -> usize {
        self.analyzers
            .iter()
            .map(|a| a.analyzer.count_matches(graph))
            .sum()
    }

    /// The distinct (analyzer index, source) pairs the analysis scans.
    pub fn unique_sources(&self, graph: &BuildGraph) -> HashSet<(usize, PathBuf)> {
        self.analyzers
            .iter()
            .enumerate()
            .flat_map(|(index, a)| {
                a.analyzer
                    .matching_sources(graph)
                    .into_iter()
                    .map(move |source| (index, source))
            })
            .collect()
    }

    /// Predict what scanning one (analyzer, source) pair will cost. An
    /// analyzer that always rescans never hits the cache.
    pub fn classify(
        &self,
        ctx: &crate::build_context::BuildContext,
        index: usize,
        source: &Path,
    ) -> ClassifyResult {
        let a = &self.analyzers[index];
        if a.analyzer.always_rescan() {
            return ClassifyResult::Miss;
        }
        self.deps_cache.classify(ctx, a.id(), source)
    }

    pub const fn stats(&self) -> &DepsCacheStats {
        self.deps_cache.stats()
    }

    /// Analyze every product of the graph: each analyzer in turn scans each
    /// distinct source it matches once and fans the result out to every
    /// product that matched it. Ticks `progress` once per product.
    ///
    /// A source that does not exist yet is the output of a product that has
    /// not run (a clean checkout, which is what CI builds every run). It
    /// contributes nothing now; [`reanalyze`](Self::reanalyze) scans it
    /// once its producer has run.
    pub fn analyze_graph(
        &mut self,
        ctx: &crate::build_context::BuildContext,
        graph: &mut BuildGraph,
        progress: &ProgressBar,
    ) -> Result<()> {
        for index in 0..self.analyzers.len() {
            // Group product ids by source so each unique source is scanned
            // once, then fan the result out to every product that matched.
            let mut by_source: BTreeMap<PathBuf, Vec<usize>> = BTreeMap::new();
            for p in graph.products() {
                if let Some(source) = self.analyzers[index].analyzer.match_product(p) {
                    by_source.entry(source).or_default().push(p.id);
                }
            }
            for (source, product_ids) in &by_source {
                progress.set_message(format!(
                    "[{}] {}",
                    self.analyzers[index].iname,
                    source.display()
                ));
                if source.exists() {
                    let result = self.scan_source(ctx, index, source)?;
                    let piece = joined_hash_pieces(&result);
                    for &id in product_ids {
                        graph.add_inputs(id, &result.deps);
                        if let Some(piece) = &piece {
                            graph.extend_cache_key(id, piece);
                        }
                    }
                }
                progress.inc(product_ids.len() as u64);
            }
        }
        self.flush_deps_cache();
        Ok(())
    }

    /// Redo the analysis of every product whose analysis read a file that
    /// one of `changed` (products just built or restored) wrote: the source
    /// an analyzer scans, or a dependency it found (a generated header can
    /// include further headers). Returns whether any product's inputs or
    /// cache key moved — the caller then re-links the graph, since a new
    /// input may be a not-yet-built output.
    ///
    /// A redone product is reset to its declared inputs and analyzed by
    /// every analyzer in graph-time order, so it ends up identical to what
    /// the next build will construct from scratch.
    pub fn reanalyze(
        &mut self,
        ctx: &crate::build_context::BuildContext,
        graph: &mut BuildGraph,
        changed: &[usize],
    ) -> Result<bool> {
        if self.analyzers.is_empty() {
            return Ok(false);
        }
        let rewritten: HashSet<PathBuf> = changed
            .iter()
            .filter_map(|&id| graph.get_product(id))
            .flat_map(|p| p.outputs.iter().cloned())
            .collect();
        if rewritten.is_empty() {
            return Ok(false);
        }
        let targets: BTreeSet<usize> = graph
            .products()
            .iter()
            .filter(|p| {
                p.analyzer_inputs().iter().any(|i| rewritten.contains(i))
                    || self.analyzers.iter().any(|a| {
                        a.analyzer
                            .match_product(p)
                            .is_some_and(|source| rewritten.contains(&source))
                    })
            })
            .map(|p| p.id)
            .collect();

        let mut moved = false;
        for id in targets {
            let before = graph
                .get_product(id)
                .map(|p| (p.inputs.clone(), p.cache_key.clone()));
            graph.reset_analysis(id);
            let mut pieces: Vec<String> = Vec::new();
            for index in 0..self.analyzers.len() {
                let product = graph
                    .get_product(id)
                    .expect(crate::errors::INVALID_PRODUCT_ID);
                let Some(source) = self.analyzers[index].analyzer.match_product(product) else {
                    continue;
                };
                if !source.exists() {
                    continue;
                }
                let result = self.scan_source(ctx, index, &source)?;
                graph.add_inputs(id, &result.deps);
                pieces.extend(joined_hash_pieces(&result));
            }
            graph.set_analyzer_pieces(id, pieces);
            let after = graph
                .get_product(id)
                .map(|p| (p.inputs.clone(), p.cache_key.clone()));
            moved |= before != after;
        }
        self.flush_deps_cache();
        Ok(moved)
    }

    /// Write the entries this pass staged, in one transaction (see
    /// `DepsCache::flush`). A failure is a warning, as a failed write always
    /// was: the cache only saves rescans, and the next build rescans.
    fn flush_deps_cache(&mut self) {
        if let Err(e) = self.deps_cache.flush() {
            crate::output::warn(&format!("failed to write the dependency cache: {e:#}"));
        }
    }

    /// The dependencies of one source: from the deps cache when valid,
    /// otherwise scanned and stored. The source checksum is taken before
    /// the scan so a mid-scan edit can't pair the new content's checksum
    /// with the old content's dependencies.
    fn scan_source(
        &mut self,
        ctx: &crate::build_context::BuildContext,
        index: usize,
        source: &Path,
    ) -> Result<ScanResult> {
        let Self {
            analyzers,
            deps_cache,
            file_index,
        } = self;
        let a = &analyzers[index];
        let cached = !a.analyzer.always_rescan();
        if cached {
            if let Some(deps) = deps_cache.get(ctx, a.id(), source) {
                return Ok(ScanResult::deps(deps, Vec::new()));
            }
        } else {
            // Matches the pre-scan prediction (`classify`), which counts
            // every always-rescan source as a rescan.
            deps_cache.count_uncached_scan();
        }
        let source_checksum = DepsCache::source_checksum(ctx, source)?;
        let result = a.analyzer.scan(ctx, source, file_index)?;
        if cached && !result.hash_pieces.is_empty() {
            anyhow::bail!(
                "analyzer '{}' returned hash pieces for {} but caches its results; \
                 hash pieces are not cached, so it must set always_rescan",
                a.iname,
                source.display()
            );
        }
        // A probe that exists was not what resolution settled on, so it was
        // never really a miss; storing it would void the entry at once.
        let absent: Vec<PathBuf> = result
            .absent
            .iter()
            .filter(|path| std::fs::symlink_metadata(path).is_err())
            .cloned()
            .collect();
        // A dependency that does not exist yet is a file another product
        // will generate. An entry naming it cannot be validated (it has no
        // checksum to compare), so none is stored; the source is scanned
        // again, mid-build once the file is generated or on the next build.
        if result.deps.iter().any(|dep| !dep.exists()) {
            return Ok(result);
        }
        // Stored even when always rescanning, so `analyzer show` can report
        // what was discovered. Hash pieces are never stored.
        if let Err(e) = deps_cache.set(ctx, a.id(), source, source_checksum, &result.deps, &absent)
        {
            crate::output::warn(&format!(
                "failed to cache dependencies for {}: {}",
                source.display(),
                e
            ));
        }
        Ok(result)
    }
}

impl ActiveAnalyzer {
    fn id(&self) -> AnalyzerId<'_> {
        AnalyzerId {
            iname: &self.iname,
            fingerprint: &self.fingerprint,
        }
    }
}

/// The paths a search probed before `found`: every candidate up to (not
/// including) the first that matched, or all of them when none did. These
/// are a scan's `absent` paths — any of them appearing changes the answer.
pub fn probed_before(candidates: &[PathBuf], found: Option<&PathBuf>) -> Vec<PathBuf> {
    candidates
        .iter()
        .take_while(|c| Some(*c) != found)
        .cloned()
        .collect()
}

/// `fingerprint_parts` for an analyzer whose resolution depends on its
/// config alone: the config, serialized.
pub fn config_fingerprint<T: serde::Serialize>(config: &T) -> Result<Vec<String>> {
    Ok(vec![
        toml::to_string(config).context("Failed to serialize analyzer config")?,
    ])
}
